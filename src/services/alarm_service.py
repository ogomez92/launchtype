import json
import threading
import uuid
from datetime import datetime
from os.path import exists

from helpers.alert_notifier import fire_alert
from helpers.json_storage import atomic_write_json


class AlarmService:
    """Manages time-of-day alarms.

    Each alarm fires once per day at its configured hour:minute (24h) while it
    is enabled. Alarms are persisted to ``alarms.json`` so they survive
    restarts. A background thread checks the wall clock once a minute and fires
    any enabled alarm whose time has arrived.
    """

    def __init__(self, alarms_file="alarms.json"):
        self.alarms_file = alarms_file
        self.alarms = []
        self._last_fired = {}  # alarm id -> "YYYY-MM-DD HH:MM" guard against re-firing
        self._load()

        self._stop_event = threading.Event()
        self._thread = threading.Thread(target=self._watch, daemon=True)
        self._thread.start()

    def _load(self):
        if not exists(self.alarms_file):
            self._sync()
            return
        try:
            with open(self.alarms_file, "r", encoding="utf-8") as f:
                self.alarms = json.loads(f.read())
        except (OSError, ValueError):
            self.alarms = []

    def _sync(self):
        atomic_write_json(self.alarms_file, self.alarms)

    def add_alarm(self, title, description, hour, minute, sound):
        self.alarms.append(
            {
                "id": str(uuid.uuid4()),
                "type": "alarm",
                "title": title,
                "description": description,
                "hour": int(hour),
                "minute": int(minute),
                "sound": sound,
                "enabled": True,
            }
        )
        self._sync()

    def toggle(self, alarm_id):
        """Toggle an alarm's activation state. Returns the new enabled state."""
        for alarm in self.alarms:
            if alarm["id"] == alarm_id:
                alarm["enabled"] = not alarm.get("enabled", False)
                self._sync()
                return alarm["enabled"]
        return None

    def remove(self, alarm_id):
        self.alarms = [a for a in self.alarms if a["id"] != alarm_id]
        self._last_fired.pop(alarm_id, None)
        self._sync()

    def get_items(self):
        items = []
        for alarm in self.alarms:
            state = _("on") if alarm.get("enabled") else _("off")
            name = _("{title} - {hour:02d}:{minute:02d} ({state})").format(
                title=alarm["title"],
                hour=alarm["hour"],
                minute=alarm["minute"],
                state=state,
            )
            items.append(
                {
                    "name": name,
                    "shortcut": "",
                    "id": alarm["id"],
                    "type": "alarm",
                }
            )
        return items

    def _watch(self):
        while not self._stop_event.is_set():
            now = datetime.now()
            key = now.strftime("%Y-%m-%d %H:%M")
            for alarm in self.alarms:
                if not alarm.get("enabled"):
                    continue
                if alarm["hour"] == now.hour and alarm["minute"] == now.minute:
                    if self._last_fired.get(alarm["id"]) != key:
                        self._last_fired[alarm["id"]] = key
                        fire_alert(alarm)
            # Check roughly twice a minute so we never miss the target minute.
            self._stop_event.wait(20)

    def stop(self):
        self._stop_event.set()
