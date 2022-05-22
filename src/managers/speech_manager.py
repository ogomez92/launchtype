import accessible_output2.outputs.auto


class SpeechManager:
    def __init__(self):
        self.output_method = accessible_output2.outputs.auto.Auto()

    def output(self, text, interrupt=True):
        self.output_method.output(text, interrupt)

    def speak(self, text, interrupt=True):
        self.output_method.speak(text, interrupt)
