import os
import winsound
from src.language_handler import get_bundle_path


class SoundPlayer:
    enabled = True

    @staticmethod
    def play(filename):
        if not SoundPlayer.enabled:
            return
        sounds_path = os.path.join(get_bundle_path(), 'sounds', f"{filename}.wav")
        winsound.PlaySound(sounds_path, winsound.SND_ASYNC)
