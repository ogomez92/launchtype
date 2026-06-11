import json
import threading
import time
import uuid
from os.path import exists

from helpers.alert_notifier import fire_alert
from helpers.json_storage import atomic_write_json


def _format_remaining(seconds):
    """Render a remaining-seconds count as ``M:SS`` (or ``H:MM:SS`` past an hour)."""
    hours, remainder = divmod(seconds, 3600)
    minutes, secs = divmod(remainder, 60)
    if hours:
        return "{h}:{m:02d}:{s:02d}".format(h=hours, m=minutes, s=secs)
    return "{m}:{s:02d}".format(m=minutes, s=secs)


class TimerService:
    """Manages countdown timers.

    Two flavours of timer are supported:

    * Non-repeating: fires once, X minutes after it is started. Running it again
      while it is already counting down simply resets the countdown.
    * Repeating: fires every X minutes until it is manually disabled. Running it
      toggles it on/off. Repeating timers default to on (both when created and
      when reloaded from disk).

    Timer definitions persist to ``timers.json``; their live running state lives
    only in memory (``_next_fire``). A background thread fires timers whose
    deadline has passed.
    """

    def __init__(self, timers_file="timers.json"):
        self.timers_file = timers_file
        self.timers = []
        self._next_fire = {}  # timer id -> epoch deadline, or None when inactive
        self._load()

        self._stop_event = threading.Event()
        self._thread = threading.Thread(target=self._watch, daemon=True)
        self._thread.start()

    def _load(self):
        if not exists(self.timers_file):
            self._sync()
            return
        try:
            with open(self.timers_file, "r", encoding="utf-8") as f:
                self.timers = json.loads(f.read())
        except (OSError, ValueError):
            self.timers = []
        # Repeating timers default to on whenever they are loaded.
        for timer in self.timers:
            if timer.get("repeating"):
                self._next_fire[timer["id"]] = time.time() + timer["minutes"] * 60

    def _sync(self):
        atomic_write_json(self.timers_file, self.timers)

    def add_timer(self, title, description, minutes, repeating, sound):
        timer = {
            "id": str(uuid.uuid4()),
            "type": "timer",
            "title": title,
            "description": description,
            "minutes": int(minutes),
            "repeating": bool(repeating),
            "sound": sound,
        }
        self.timers.append(timer)
        self._sync()
        # Repeating timers start on by default.
        if timer["repeating"]:
            self._next_fire[timer["id"]] = time.time() + timer["minutes"] * 60

    def toggle(self, timer_id):
        """Run/toggle a timer. Returns the new active state (True/False)."""
        for timer in self.timers:
            if timer["id"] != timer_id:
                continue
            if timer.get("repeating"):
                # Repeating timers toggle on/off.
                if self._next_fire.get(timer_id):
                    self._next_fire[timer_id] = None
                    return False
                self._next_fire[timer_id] = time.time() + timer["minutes"] * 60
                return True
            # Non-repeating timers (re)start the countdown each time they run.
            self._next_fire[timer_id] = time.time() + timer["minutes"] * 60
            return True
        return None

    def is_active(self, timer_id):
        return bool(self._next_fire.get(timer_id))

    def remaining(self, timer_id):
        """Seconds left until the timer next fires, or None when inactive."""
        deadline = self._next_fire.get(timer_id)
        if not deadline:
            return None
        return max(0, int(round(deadline - time.time())))

    def remove(self, timer_id):
        self.timers = [t for t in self.timers if t["id"] != timer_id]
        self._next_fire.pop(timer_id, None)
        self._sync()

    def get_items(self):
        items = []
        for timer in self.timers:
            remaining = self.remaining(timer["id"])
            if remaining is not None:
                verb = _("until repeat") if timer.get("repeating") else _("left")
                state = _("running, {time} {verb}").format(
                    time=_format_remaining(remaining), verb=verb
                )
            else:
                state = _("stopped")
            if timer.get("repeating"):
                descriptor = _("every {minutes} min, repeating").format(
                    minutes=timer["minutes"]
                )
            else:
                descriptor = _("{minutes} min").format(minutes=timer["minutes"])
            name = _("{title} - {descriptor} ({state})").format(
                title=timer["title"], descriptor=descriptor, state=state
            )
            items.append(
                {
                    "name": name,
                    "shortcut": "",
                    "id": timer["id"],
                    "type": "timer",
                }
            )
        return items

    def _watch(self):
        while not self._stop_event.is_set():
            now = time.time()
            for timer in self.timers:
                deadline = self._next_fire.get(timer["id"])
                if deadline and now >= deadline:
                    fire_alert(timer)
                    if timer.get("repeating"):
                        self._next_fire[timer["id"]] = now + timer["minutes"] * 60
                    else:
                        self._next_fire[timer["id"]] = None
            self._stop_event.wait(1)

    def stop(self):
        self._stop_event.set()
