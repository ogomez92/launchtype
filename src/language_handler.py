import gettext
import locale
import os
import sys


def get_bundle_path():
    """Get the directory where the application is running from."""
    if getattr(sys, 'frozen', False):
        # Running as PyInstaller bundle
        # Use _MEIPASS which points to the temp folder where PyInstaller extracts files
        return getattr(sys, '_MEIPASS', os.path.dirname(sys.executable))
    else:
        # Running as script - go up one level from src/ to project root
        return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def initialize():
    # Try multiple methods to get the system locale
    current_locale = None

    # First, try getdefaultlocale() which is more reliable in PyInstaller
    try:
        current_locale, encoding = locale.getdefaultlocale()
    except:
        pass

    # If that fails, try getlocale()
    if not current_locale:
        try:
            current_locale, encoding = locale.getlocale()
        except:
            pass

    # If still no locale, try with LC_MESSAGES category (more reliable on some systems)
    if not current_locale:
        try:
            current_locale, encoding = locale.getlocale(locale.LC_MESSAGES)
        except:
            pass

    # Final fallback to English
    if not current_locale:
        current_locale = "en"

    language_code = current_locale.split("_")[0]
    locale_path = os.path.join(get_bundle_path(), 'locale')
    translations = gettext.translation('launchtype', localedir=locale_path, languages=[language_code], fallback=True)
    translations.install()
