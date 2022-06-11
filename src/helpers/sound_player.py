import wx


class SoundPlayer:
    @staticmethod
    def play(filename):
        sound = wx.Sound(f"./sounds/{filename}.wav")
        sound.Stop()
        sound.Play(wx.SOUND_ASYNC)