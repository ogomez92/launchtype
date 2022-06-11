import accessible_output2.outputs.auto


class SpeechService:
    output_method = None

    @staticmethod
    def initialize():
        SpeechService.output_method = accessible_output2.outputs.auto.Auto()

    @staticmethod
    def speak(text, interrupt=True):
        SpeechService.output_method.speak(text, interrupt)
