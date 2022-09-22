import accessible_output2.outputs.auto


class SpeechService:
    output_method = None
    fallback_method = None


    @staticmethod
    def initialize():
        try:
            SpeechService.fallback_method = accessible_output2.outputs.nvda.NVDA()
            SpeechService.output_method = accessible_output2.outputs.auto.Auto()
        except Exception as e:
            print('cannot automatically get output method, thank you accessible output2!')
            SpeechService.output_method = accessible_output2.outputs.nvda.NVDA()
            pass

    @staticmethod
    def speak(text, interrupt=True):
        try:
            SpeechService.output_method.speak(text, interrupt)
        except:
            SpeechService.fallback_method.speak(text, interrupt)
            pass
