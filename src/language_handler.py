import gettext
import locale

def initialize():
    current_locale, encoding = locale.getlocale()
    language_code = current_locale.split("_")[0]
    translations = gettext.translation('launchtype', localedir='locale', languages=[language_code], fallback = True)
    translations.install()
