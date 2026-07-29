#!/usr/bin/env python3
"""Artupy's safe, dependency-free KeyLab Essential mk3 OLED bridge.

This utility talks to Windows' legacy MIDI API (winmm).  It only sends
temporary DAW-mode SysEx messages; it never writes firmware or user memories.
"""

from __future__ import annotations

import argparse
from contextlib import ExitStack
import ctypes
from ctypes import wintypes
import sys
import time
from dataclasses import dataclass
from typing import Sequence


if sys.platform != "win32":
    raise SystemExit("Esta herramienta solo funciona en Windows.")


MAXPNAMELEN = 32
MMSYSERR_NOERROR = 0
MIDIERR_STILLPLAYING = 65
MHDR_DONE = 0x00000001
SYSEX_PREFIX = bytes.fromhex("F0 00 20 6B 7F 42")
SYSEX_SUFFIX = b"\xF7"


class MIDIOUTCAPSW(ctypes.Structure):
    _fields_ = [
        ("wMid", wintypes.WORD),
        ("wPid", wintypes.WORD),
        ("vDriverVersion", wintypes.UINT),
        ("szPname", wintypes.WCHAR * MAXPNAMELEN),
        ("wTechnology", wintypes.WORD),
        ("wVoices", wintypes.WORD),
        ("wNotes", wintypes.WORD),
        ("wChannelMask", wintypes.WORD),
        ("dwSupport", wintypes.DWORD),
    ]


DWORD_PTR = ctypes.c_size_t


class MIDIHDR(ctypes.Structure):
    _fields_ = [
        ("lpData", ctypes.c_void_p),
        ("dwBufferLength", wintypes.DWORD),
        ("dwBytesRecorded", wintypes.DWORD),
        ("dwUser", DWORD_PTR),
        ("dwFlags", wintypes.DWORD),
        ("lpNext", ctypes.c_void_p),
        ("reserved", DWORD_PTR),
        ("dwOffset", wintypes.DWORD),
        ("dwReserved", DWORD_PTR * 8),
    ]


@dataclass(frozen=True)
class MidiPort:
    device_id: int
    name: str


class MidiError(RuntimeError):
    pass


class WinMM:
    def __init__(self) -> None:
        self.dll = ctypes.WinDLL("winmm")
        self.dll.midiOutGetNumDevs.restype = wintypes.UINT
        self.dll.midiOutGetDevCapsW.argtypes = [
            DWORD_PTR,
            ctypes.POINTER(MIDIOUTCAPSW),
            wintypes.UINT,
        ]
        self.dll.midiOutGetDevCapsW.restype = wintypes.UINT
        self.dll.midiOutOpen.argtypes = [
            ctypes.POINTER(ctypes.c_void_p),
            wintypes.UINT,
            DWORD_PTR,
            DWORD_PTR,
            wintypes.DWORD,
        ]
        self.dll.midiOutOpen.restype = wintypes.UINT
        self.dll.midiOutPrepareHeader.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(MIDIHDR),
            wintypes.UINT,
        ]
        self.dll.midiOutPrepareHeader.restype = wintypes.UINT
        self.dll.midiOutLongMsg.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(MIDIHDR),
            wintypes.UINT,
        ]
        self.dll.midiOutLongMsg.restype = wintypes.UINT
        self.dll.midiOutUnprepareHeader.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(MIDIHDR),
            wintypes.UINT,
        ]
        self.dll.midiOutUnprepareHeader.restype = wintypes.UINT
        self.dll.midiOutClose.argtypes = [ctypes.c_void_p]
        self.dll.midiOutClose.restype = wintypes.UINT
        self.dll.midiOutGetErrorTextW.argtypes = [
            wintypes.UINT,
            wintypes.LPWSTR,
            wintypes.UINT,
        ]
        self.dll.midiOutGetErrorTextW.restype = wintypes.UINT

    def error_text(self, result: int) -> str:
        buffer = ctypes.create_unicode_buffer(256)
        status = self.dll.midiOutGetErrorTextW(result, buffer, len(buffer))
        return buffer.value if status == MMSYSERR_NOERROR else f"código {result}"

    def check(self, result: int, action: str) -> None:
        if result != MMSYSERR_NOERROR:
            raise MidiError(f"{action}: {self.error_text(result)}")

    def ports(self) -> list[MidiPort]:
        ports: list[MidiPort] = []
        for device_id in range(self.dll.midiOutGetNumDevs()):
            caps = MIDIOUTCAPSW()
            result = self.dll.midiOutGetDevCapsW(
                device_id, ctypes.byref(caps), ctypes.sizeof(caps)
            )
            self.check(result, f"No se pudo consultar el puerto {device_id}")
            ports.append(MidiPort(device_id, caps.szPname))
        return ports


class MidiOutput:
    def __init__(self, api: WinMM, port: MidiPort) -> None:
        self.api = api
        self.port = port
        self.handle = ctypes.c_void_p()

    def __enter__(self) -> "MidiOutput":
        result = self.api.dll.midiOutOpen(
            ctypes.byref(self.handle), self.port.device_id, 0, 0, 0
        )
        self.api.check(result, f"No se pudo abrir [{self.port.device_id}] {self.port.name}")
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        if self.handle:
            result = self.api.dll.midiOutClose(self.handle)
            if result != MMSYSERR_NOERROR and exc_value is None:
                raise MidiError(f"No se pudo cerrar el puerto: {self.api.error_text(result)}")

    def send_sysex(self, message: bytes, timeout: float = 2.0) -> None:
        validate_sysex(message)
        data = ctypes.create_string_buffer(message, len(message))
        header = MIDIHDR()
        header.lpData = ctypes.cast(data, ctypes.c_void_p)
        header.dwBufferLength = len(message)
        header.dwBytesRecorded = len(message)
        header_size = ctypes.sizeof(header)

        result = self.api.dll.midiOutPrepareHeader(
            self.handle, ctypes.byref(header), header_size
        )
        self.api.check(result, "No se pudo preparar el mensaje SysEx")
        try:
            result = self.api.dll.midiOutLongMsg(
                self.handle, ctypes.byref(header), header_size
            )
            self.api.check(result, "No se pudo enviar el mensaje SysEx")

            deadline = time.monotonic() + timeout
            while not header.dwFlags & MHDR_DONE:
                if time.monotonic() >= deadline:
                    raise MidiError("El envío SysEx no terminó dentro del tiempo esperado")
                time.sleep(0.01)
        finally:
            deadline = time.monotonic() + timeout
            while True:
                result = self.api.dll.midiOutUnprepareHeader(
                    self.handle, ctypes.byref(header), header_size
                )
                if result == MMSYSERR_NOERROR:
                    break
                if result != MIDIERR_STILLPLAYING or time.monotonic() >= deadline:
                    raise MidiError(
                        "No se pudo liberar el mensaje SysEx: "
                        + self.api.error_text(result)
                    )
                time.sleep(0.01)


def sysex(payload: bytes) -> bytes:
    if any(byte > 0x7F for byte in payload):
        raise ValueError("El contenido SysEx debe usar datos MIDI de 7 bits")
    return SYSEX_PREFIX + payload + SYSEX_SUFFIX


def ascii_7bit(text: str, maximum: int = 18) -> bytes:
    encoded = text.encode("ascii", errors="strict")
    if not encoded:
        raise ValueError("El texto no puede estar vacío")
    if len(encoded) > maximum:
        raise ValueError(f"El texto supera el máximo de {maximum} caracteres")
    if b"\x00" in encoded:
        raise ValueError("El texto no puede contener NUL")
    return encoded


def screen_two_lines(line_1: str, line_2: str, transient: bool = False) -> bytes:
    payload = (
        bytes.fromhex("04 01 60 12 01")
        + ascii_7bit(line_1)
        + bytes.fromhex("00 02")
        + ascii_7bit(line_2)
        + b"\x00"
        + bytes([int(transient)])
    )
    return sysex(payload)


def screen_header(text: str) -> bytes:
    return sysex(bytes.fromhex("04 01 60 01 02") + ascii_7bit(text) + b"\x00\x00")


CONNECT = sysex(bytes.fromhex("02 0F 40 5A 01"))
DISCONNECT = sysex(bytes.fromhex("02 0F 40 5A 00"))
CLEAR_SCREEN = sysex(bytes.fromhex("04 01 60 61"))


def select_preset(index: int) -> bytes:
    if not 0 <= index <= 7:
        raise ValueError("El índice de programa debe estar entre 0 y 7")
    return sysex(bytes.fromhex("21 11 40 02 00") + bytes([index]))


ARTURIA_PRESET = select_preset(0)
DAW_PRESET = select_preset(1)


def validate_sysex(message: bytes) -> None:
    if len(message) < 2 or message[0] != 0xF0 or message[-1] != 0xF7:
        raise ValueError("Mensaje SysEx inválido")
    if any(byte > 0x7F for byte in message[1:-1]):
        raise ValueError("SysEx contiene un byte de datos fuera del rango MIDI")


def choose_port(ports: Sequence[MidiPort], selector: str) -> MidiPort:
    try:
        numeric_id = int(selector)
    except ValueError:
        numeric_id = None

    if numeric_id is not None:
        matches = [port for port in ports if port.device_id == numeric_id]
    else:
        needle = selector.casefold()
        matches = [port for port in ports if needle in port.name.casefold()]

    if not matches:
        raise MidiError(f"No se encontró un puerto de salida que coincida con {selector!r}")
    if len(matches) > 1:
        options = ", ".join(f"[{port.device_id}] {port.name}" for port in matches)
        raise MidiError(f"Selección ambigua; usa el número del puerto: {options}")
    return matches[0]


def looks_like_keylab(port: MidiPort) -> bool:
    name = port.name.casefold()
    return (
        "keylab" in name
        or "arturia" in name
        or "kl essential" in name
    )


def dump_messages(messages: Sequence[tuple[str, bytes]]) -> None:
    for label, message in messages:
        print(f"{label:12} {message.hex(' ').upper()}")


def run_demo(
    display_port: MidiPort,
    control_port: MidiPort,
    execute: bool,
    seconds: float,
) -> None:
    messages = [
        ("preset-daw", DAW_PRESET),
        ("connect", CONNECT),
        ("header", screen_header("KEYLAB 61 MK3")),
        ("screen", screen_two_lines("DOOM", "READY TO RIP")),
    ]
    cleanup = [
        ("clear", CLEAR_SCREEN),
        ("disconnect", DISCONNECT),
        ("preset-art", ARTURIA_PRESET),
    ]

    print(
        f"Puerto de pantalla: [{display_port.device_id}] {display_port.name}\n"
        f"Puerto de control:  [{control_port.device_id}] {control_port.name}"
    )
    if not execute:
        print("DRY-RUN: no se enviará nada. Mensajes preparados:")
        dump_messages(messages + cleanup)
        return

    if not looks_like_keylab(display_port) or not looks_like_keylab(control_port):
        raise MidiError(
            "Alguno de los puertos no parece pertenecer a un KeyLab/Arturia; "
            "se canceló el envío"
        )

    api = WinMM()
    switched_to_daw = False
    connected = False
    with ExitStack() as stack:
        control = stack.enter_context(MidiOutput(api, control_port))
        if display_port.device_id == control_port.device_id:
            display = control
        else:
            display = stack.enter_context(MidiOutput(api, display_port))
        try:
            control.send_sysex(DAW_PRESET)
            switched_to_daw = True
            time.sleep(0.35)
            display.send_sysex(CONNECT)
            connected = True
            time.sleep(0.15)
            display.send_sysex(screen_header("KEYLAB 61 MK3"))
            time.sleep(0.05)
            display.send_sysex(screen_two_lines("DOOM", "READY TO RIP"))
            print(f"Prueba activa durante {seconds:g} segundos...")
            time.sleep(seconds)
        finally:
            cleanup_failures: list[str] = []
            if connected:
                for label, message in (
                    ("limpiar pantalla", CLEAR_SCREEN),
                    ("desconectar sesión DAW", DISCONNECT),
                ):
                    try:
                        display.send_sysex(message)
                    except (MidiError, OSError, ValueError) as error:
                        cleanup_failures.append(f"{label}: {error}")
                time.sleep(0.15)
            if switched_to_daw:
                try:
                    control.send_sysex(ARTURIA_PRESET)
                except (MidiError, OSError, ValueError) as error:
                    cleanup_failures.append(f"restaurar programa Arturia: {error}")
            if cleanup_failures:
                raise MidiError(
                    "Falló parte de la restauración: " + "; ".join(cleanup_failures)
                )
    print(
        "Prueba terminada; se restauraron la sesión, la pantalla "
        "y el programa Arturia."
    )


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Prueba segura de la OLED del Arturia KeyLab Essential mk3"
    )
    parser.add_argument(
        "--list", action="store_true", help="listar puertos MIDI de salida"
    )
    parser.add_argument(
        "--demo", action="store_true", help="preparar la demostración DOOM de artupy"
    )
    parser.add_argument(
        "--port",
        help="puerto de integración terminado en MIDI: ID numérico o nombre",
    )
    parser.add_argument(
        "--control-port",
        help=(
            "puerto global alternativo (opción avanzada; por defecto se usa --port)"
        ),
    )
    parser.add_argument(
        "--execute",
        action="store_true",
        help="enviar realmente (sin esto, --demo es solo dry-run)",
    )
    parser.add_argument(
        "--seconds",
        type=float,
        default=5.0,
        help="duración visible de la prueba (0.5 a 30; predeterminado: 5)",
    )
    args = parser.parse_args(argv)
    if args.demo and args.port is None:
        parser.error("--demo requiere --port")
    if args.execute and not args.demo:
        parser.error("--execute solo se admite junto con --demo")
    if not 0.5 <= args.seconds <= 120:
        parser.error("--seconds debe estar entre 0.5 y 120")
    if not args.list and not args.demo:
        args.list = True
    return args


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        api = WinMM()
        ports = api.ports()
        if args.list:
            if not ports:
                print("Windows no reportó puertos MIDI de salida.")
            else:
                print("Puertos MIDI de salida:")
                for port in ports:
                    marker = "  <posible KeyLab>" if looks_like_keylab(port) else ""
                    print(f"  [{port.device_id}] {port.name}{marker}")
        if args.demo:
            display_port = choose_port(ports, args.port)
            control_port = choose_port(ports, args.control_port or args.port)
            run_demo(display_port, control_port, args.execute, args.seconds)
        return 0
    except (MidiError, OSError, ValueError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
