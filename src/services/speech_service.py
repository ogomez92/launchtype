import accessible_output2.outputs.auto


class SpeechService:
    def initialize(self):
        self.output_method = accessible_output2.outputs.auto.Auto()

    def speak(text, interrupt=True):
        self.output_method.speak(text, interrupt)
