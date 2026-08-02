# Acceso y despliegue

El repositorio local es la única fuente de verdad. `/home/kalex/rackforge/current`
en la Raspberry es una copia desplegada y puede reconstruirse en cualquier
momento.

## SSH

La clave privada permanece fuera del repositorio:

```text
C:\Users\kalex\.ssh\rackforge_ed25519
```

Copiar `ssh_config.example` a la configuración SSH local permite usar:

```powershell
ssh rackforge
```

La autenticación por contraseña permanece como vía de recuperación del usuario,
pero las herramientas del proyecto exigen autenticación no interactiva por
clave.

## Herramientas

- `connect.ps1`: abre una terminal remota.
- `health.ps1`: muestra salud, temperatura, throttling, USB y ALSA.
- `sync.ps1`: empaqueta el repositorio, excluye artefactos locales y lo despliega
  en `current/`.
- `install-nuked-roms.ps1`: valida y transfiere ROMs propias fuera de Git.
- `bootstrap.sh`: reproduce hostname, paquetes, Rust y directorios base.

Ninguna herramienta copia claves, contraseñas, bancos o estado runtime al
repositorio.
