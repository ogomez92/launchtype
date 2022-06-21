import accessible_output2.outputs.auto


class SpeechService:
    output_method = None

    @staticmethod
    def initialize():
        try:
            SpeechService.output_method = accessible_output2.outputs.auto.Auto()
        except:
            pass

    @staticmethod
    def speak(text, interrupt=True):
        try:
            SpeechService.output_method.speak(text, interrupt)
        except:
            pass
