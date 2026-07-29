#include <RtMidi.h>
#include <SDL.h>

#include <algorithm>
#include <cctype>
#include <exception>
#include <iostream>
#include <string>
#include <vector>

namespace {

std::string folded(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return value;
}

int find_midi(const std::string& needle) {
    RtMidiIn midi(RtMidi::UNSPECIFIED, "RackForge Nuked probe", 128);
    const auto wanted = folded(needle);
    std::vector<unsigned int> matches;

    for (unsigned int index = 0; index < midi.getPortCount(); ++index) {
        if (folded(midi.getPortName(index)).find(wanted) != std::string::npos) {
            matches.push_back(index);
        }
    }

    if (matches.size() != 1) {
        std::cerr << "expected one MIDI match for '" << needle << "', got "
                  << matches.size() << '\n';
        return 2;
    }

    std::cout << matches.front() << '\n';
    return 0;
}

int list_devices() {
    RtMidiIn midi(RtMidi::UNSPECIFIED, "RackForge Nuked probe", 128);
    std::cout << "MIDI inputs:\n";
    for (unsigned int index = 0; index < midi.getPortCount(); ++index) {
        std::cout << "  [" << index << "] " << midi.getPortName(index) << '\n';
    }

    if (SDL_Init(SDL_INIT_AUDIO) != 0) {
        std::cerr << "SDL audio init failed: " << SDL_GetError() << '\n';
        return 1;
    }

    std::cout << "SDL audio outputs:\n";
    const int count = SDL_GetNumAudioDevices(0);
    for (int index = 0; index < count; ++index) {
        std::cout << "  [" << index << "] " << SDL_GetAudioDeviceName(index, 0)
                  << '\n';
    }
    SDL_Quit();
    return 0;
}

}  // namespace

int main(int argc, char* argv[]) {
    try {
        if (argc == 3 && std::string(argv[1]) == "--find-midi") {
            return find_midi(argv[2]);
        }
        if (argc != 1) {
            std::cerr << "usage: rackforge-nuked-probe [--find-midi NAME]\n";
            return 2;
        }
        return list_devices();
    } catch (const std::exception& error) {
        std::cerr << "probe failed: " << error.what() << '\n';
        return 1;
    }
}
