"""Key/pedal state for the shared physical diagnostic's assumed damper ramp."""
from concert_grand_damped_coast import engagement


class DamperControl:
    def __init__(self, keys, pedal=False):
        self.keys = {key: False for key in keys}
        # Pedal-up rest starts closed, rather than lowering from an open patch.
        self.changed = {key: -.03 for key in keys}
        self.pedal = pedal

    def event(self, kind, key, now):
        if kind == 'on' or kind == 'off':
            self.keys[key] = kind == 'on'
            self.changed[key] = now
        elif kind == 'pedal_off':
            if self.pedal:
                self.pedal = False
                for k in self.keys:
                    if not self.keys[k]:
                        self.changed[k] = now
        else:
            raise ValueError('unknown damper control event')

    def levels(self, now):
        return {key: 0. if self.pedal or held else engagement(now-self.changed[key])
                for key, held in self.keys.items()}
