import os
import winsound
from src.language_handler import get_bundle_path


class SoundPlayer:
    @staticmethod
    def play(filename):
        sounds_path = os.path.join(get_bundle_path(), 'sounds', f"{filename}.wav")
        winsound.PlaySound(sounds_path, winsound.SND_ASYNC)
