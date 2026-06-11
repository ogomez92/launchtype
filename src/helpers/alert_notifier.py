import os
import winsound

from services.speech_service import SpeechService


def fire_alert(item):
    """Announce an alarm/timer via the screen reader and play its sound.

    The item is expected to provide a ``title``, an optional ``description``
    and an optional ``sound`` (a path to a .wav file anywhere on disk). When no
    custom sound is set, or it cannot be played, NVDA still speaks the message
    and we play the default system beep so the user gets audible feedback.
    """
    title = item.get("title", "") or item.get("name", "")
    description = item.get("description", "")
    message = f"{title}: {description}" if description else title

    try:
        SpeechService.speak(message)
    except Exception:
        pass

    sound = item.get("sound", "")
    if sound and os.path.exists(sound):
        try:
            winsound.PlaySound(sound, winsound.SND_ASYNC)
            return
        except Exception:
            pass

    # No custom sound (or it failed to play): fall back to the system beep.
    winsound.MessageBeep()
