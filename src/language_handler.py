import gettext
import locale

def initialize():
    current_locale, encoding = locale.getlocale()

    if not current_locale:
        current_locale = "en"
        
    language_code = current_locale.split("_")[0]
    translations = gettext.translation('launchtype', localedir='locale', languages=[language_code], fallback = True)
    translations.install()
