import winsound


class SoundPlayer:
    @staticmethod
    def play(filename):
        assembled_filename = f"sounds/{filename}.wav"
        winsound.PlaySound(assembled_filename, winsound.SND_ASYNC)
