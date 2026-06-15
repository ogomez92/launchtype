"""Realtime data lookups for the `+` mode.

Every source is a free, keyless HTTP API queried with the standard library
`urllib` so no new dependency is added:

- CoinGecko: bitcoin and ethereum prices in euros
- Frankfurter (ECB reference rates): euros to US dollars
- Yahoo Finance chart API: brent crude, gold and the IBEX 35 index
- ipinfo.io + Open-Meteo: current weather at the user's location
- El País / BBC RSS feeds: news headlines

Each fetcher returns a ready-to-speak sentence; failures raise
:class:`RealtimeError` carrying a human-readable reason. Fetchers run in a
background thread (see ``UIManager.fetch_realtime_value``) so they must not
touch the UI.
"""

import json
import os
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ElementTree
from datetime import datetime

from helpers.json_storage import atomic_write_json

# Network timeout in seconds for every request.
TIMEOUT = 15

# Last reading of every numeric item, so a new lookup can be compared against
# the previous one. Persisted to the working directory so the comparison spans
# app restarts; serialised access keeps concurrent background fetches safe.
HISTORY_FILE = "realtime_history.json"
_HISTORY_LOCK = threading.Lock()

# Yahoo Finance rejects requests without a browser-looking user agent.
USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) launchtype"

HEADLINE_COUNT = 5

# Used when the IP geolocation lookup fails.
FALLBACK_CITY = "Madrid"
FALLBACK_LATITUDE = 40.4168
FALLBACK_LONGITUDE = -3.7038


class RealtimeError(Exception):
    """Raised when a realtime data source cannot be fetched or parsed.

    ``code`` carries the HTTP status code when the failure was an HTTP error,
    so fetchers can give a friendlier message for specific statuses.
    """

    def __init__(self, message, code=None):
        super().__init__(message)
        self.code = code


def get_realtime_items():
    """Return the list of realtime data items for the UI."""
    definitions = [
        ("bitcoin", _("bitcoin price in euros"), "btc"),
        ("ethereum", _("ethereum price in euros"), "eth"),
        ("eur_usd", _("1000 euros in us dollars"), "usd"),
        ("brent", _("brent crude oil price"), "oil"),
        ("gold", _("gold price"), "gold"),
        ("ibex", _("ibex 35 stock index"), "ibex"),
        ("weather", _("weather at my location"), "w"),
        ("elpais", _("el país news headlines"), "news"),
        ("catalunya", _("catalunya news headlines"), "cat"),
        ("vilaweb", _("vilaweb news in catalan"), "vila"),
        ("bbc", _("bbc world news headlines"), "bbc"),
        ("claude", _("claude usage limits"), "cc"),
    ]
    return [
        {
            "name": name,
            "shortcut": shortcut,
            "key": key,
            "id": key,
            "type": "realtime",
        }
        for key, name, shortcut in definitions
    ]


def fetch_value(key):
    """Fetch the realtime value for ``key`` and return a speakable sentence."""
    fetcher = _FETCHERS.get(key)
    if fetcher is None:
        raise RealtimeError(_("Unknown realtime item."))
    return fetcher()


def _http_get(url, headers=None):
    request_headers = {"User-Agent": USER_AGENT}
    if headers:
        request_headers.update(headers)
    request = urllib.request.Request(url, headers=request_headers)
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
            return response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as error:
        raise RealtimeError(
            _("Server returned an unexpected status code: {}").format(error.code),
            code=error.code,
        )
    except urllib.error.URLError as error:
        raise RealtimeError(_("Network error: {}").format(error.reason))
    except Exception as error:  # noqa: BLE001 - surface anything else as a reason
        raise RealtimeError(_("Unexpected error: {}").format(error))


def _get_json(url, headers=None):
    try:
        return json.loads(_http_get(url, headers=headers))
    except ValueError:
        raise RealtimeError(
            _("The server returned data that could not be understood.")
        )


def _format_number(value, decimals=2):
    """Format a number for speech: no group separators, no trailing zeros."""
    rounded = round(float(value), decimals)
    if rounded == int(rounded):
        return str(int(rounded))
    return f"{rounded:.{decimals}f}".rstrip("0").rstrip(".")


def _load_history():
    try:
        with open(HISTORY_FILE, "r", encoding="utf-8") as history_file:
            data = json.load(history_file)
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def _format_elapsed(seconds):
    """Render an elapsed-seconds count as a speakable "... ago" phrase."""
    seconds = max(0, int(seconds))
    if seconds < 60:
        return _("a few seconds ago")
    minutes = seconds // 60
    if minutes < 60:
        if minutes == 1:
            return _("1 minute ago")
        return _("{count} minutes ago").format(count=minutes)
    hours = minutes // 60
    if hours < 24:
        if hours == 1:
            return _("1 hour ago")
        return _("{count} hours ago").format(count=hours)
    days = hours // 24
    if days == 1:
        return _("1 day ago")
    return _("{count} days ago").format(count=days)


def _compare_and_store(key, current, unit):
    """Compare ``current`` against the last stored reading for ``key`` and
    return a speakable change phrase, then persist the new reading.

    Returns an empty string when there is no previous reading to compare to.
    """
    current = float(current)
    now = time.time()
    with _HISTORY_LOCK:
        history = _load_history()
        previous = history.get(key)
        history[key] = {"value": current, "timestamp": now}
        atomic_write_json(HISTORY_FILE, history)

    if not isinstance(previous, dict):
        return ""
    try:
        previous_value = float(previous["value"])
        previous_time = float(previous["timestamp"])
    except (KeyError, TypeError, ValueError):
        return ""

    elapsed = _format_elapsed(now - previous_time)
    difference = current - previous_value
    if round(difference, 2) == 0:
        return _("unchanged since {elapsed}").format(elapsed=elapsed)

    amount = _format_number(abs(difference))
    if previous_value:
        percent = _format_number(abs(difference) / abs(previous_value) * 100)
    else:
        percent = _format_number(0)

    if difference > 0:
        return _(
            "up {amount} {unit} ({percent} percent) since {elapsed}"
        ).format(amount=amount, unit=unit, percent=percent, elapsed=elapsed)
    return _(
        "down {amount} {unit} ({percent} percent) since {elapsed}"
    ).format(amount=amount, unit=unit, percent=percent, elapsed=elapsed)


def _with_comparison(sentence, key, current, unit):
    """Append the change-vs-last-reading phrase to ``sentence`` when available."""
    comparison = _compare_and_store(key, current, unit)
    if comparison:
        return sentence + ", " + comparison
    return sentence


def _coingecko_price_in_euros(coin_id):
    body = _get_json(
        "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=eur".format(
            coin_id
        )
    )
    try:
        return body[coin_id]["eur"]
    except (KeyError, TypeError):
        raise RealtimeError(
            _("The server returned data that could not be understood.")
        )


def _yahoo_market_price(symbol):
    body = _get_json(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?range=1d&interval=1d".format(
            urllib.parse.quote(symbol)
        )
    )
    try:
        return body["chart"]["result"][0]["meta"]["regularMarketPrice"]
    except (KeyError, IndexError, TypeError):
        raise RealtimeError(
            _("The server returned data that could not be understood.")
        )


def _fetch_bitcoin():
    price = _coingecko_price_in_euros("bitcoin")
    sentence = _("One bitcoin is {price} euros right now").format(
        price=_format_number(price)
    )
    return _with_comparison(sentence, "bitcoin", price, _("euros"))


def _fetch_ethereum():
    price = _coingecko_price_in_euros("ethereum")
    sentence = _("One ethereum is {price} euros right now").format(
        price=_format_number(price)
    )
    return _with_comparison(sentence, "ethereum", price, _("euros"))


def _fetch_eur_usd():
    body = _get_json("https://api.frankfurter.dev/v1/latest?base=EUR&symbols=USD")
    try:
        rate = float(body["rates"]["USD"])
    except (KeyError, TypeError, ValueError):
        raise RealtimeError(
            _("The server returned data that could not be understood.")
        )
    amount = rate * 1000
    sentence = _(
        "1000 euros are {amount} us dollars, at a rate of {rate} dollars per euro"
    ).format(amount=_format_number(amount), rate=_format_number(rate, 4))
    return _with_comparison(sentence, "eur_usd", amount, _("us dollars"))


def _fetch_brent():
    price = _yahoo_market_price("BZ=F")
    sentence = _("A barrel of brent crude oil is {price} us dollars").format(
        price=_format_number(price)
    )
    return _with_comparison(sentence, "brent", price, _("us dollars"))


def _fetch_gold():
    price = _yahoo_market_price("GC=F")
    sentence = _("An ounce of gold is {price} us dollars").format(
        price=_format_number(price)
    )
    return _with_comparison(sentence, "gold", price, _("us dollars"))


def _fetch_ibex():
    points = _yahoo_market_price("^IBEX")
    sentence = _("The ibex 35 is at {points} points").format(
        points=_format_number(points)
    )
    return _with_comparison(sentence, "ibex", points, _("points"))


def _locate():
    """Geolocate the machine by IP; fall back to Madrid on any failure."""
    try:
        body = _get_json("https://ipinfo.io/json")
        latitude, longitude = body["loc"].split(",")
        city = body.get("city") or FALLBACK_CITY
        return city, float(latitude), float(longitude)
    except (RealtimeError, KeyError, ValueError, AttributeError):
        return FALLBACK_CITY, FALLBACK_LATITUDE, FALLBACK_LONGITUDE


def _weather_description(code):
    """Translate a WMO weather code into a short spoken description."""
    if code == 0:
        return _("clear sky")
    if code in (1, 2):
        return _("partly cloudy")
    if code == 3:
        return _("overcast")
    if code in (45, 48):
        return _("fog")
    if 51 <= code <= 57:
        return _("drizzle")
    if 61 <= code <= 67:
        return _("rain")
    if 71 <= code <= 77:
        return _("snow")
    if 80 <= code <= 82:
        return _("rain showers")
    if code in (85, 86):
        return _("snow showers")
    if code >= 95:
        return _("thunderstorm")
    return _("variable conditions")


def _fetch_weather():
    city, latitude, longitude = _locate()
    body = _get_json(
        "https://api.open-meteo.com/v1/forecast"
        "?latitude={}&longitude={}"
        "&current=temperature_2m,apparent_temperature,relative_humidity_2m,"
        "wind_speed_10m,weather_code"
        "&daily=temperature_2m_max,temperature_2m_min"
        "&timezone=auto&forecast_days=1".format(latitude, longitude)
    )
    try:
        current = body["current"]
        daily = body["daily"]
        return _(
            "Weather in {city}: {description}, {temperature} degrees, "
            "feels like {feels}, humidity {humidity} percent, "
            "wind {wind} kilometers per hour. "
            "Today between {minimum} and {maximum} degrees."
        ).format(
            city=city,
            description=_weather_description(int(current["weather_code"])),
            temperature=_format_number(current["temperature_2m"], 1),
            feels=_format_number(current["apparent_temperature"], 1),
            humidity=_format_number(current["relative_humidity_2m"], 0),
            wind=_format_number(current["wind_speed_10m"], 0),
            minimum=_format_number(daily["temperature_2m_min"][0], 1),
            maximum=_format_number(daily["temperature_2m_max"][0], 1),
        )
    except (KeyError, IndexError, TypeError, ValueError):
        raise RealtimeError(
            _("The server returned data that could not be understood.")
        )


def _fetch_rss_headlines(url, source):
    try:
        root = ElementTree.fromstring(_http_get(url))
    except ElementTree.ParseError:
        raise RealtimeError(
            _("The server returned data that could not be understood.")
        )

    titles = []
    for item in root.iter("item"):
        title = item.findtext("title")
        if title and title.strip():
            titles.append(title.strip())
        if len(titles) == HEADLINE_COUNT:
            break

    if not titles:
        raise RealtimeError(_("The news feed contained no headlines."))

    return _("Latest headlines from {source}: {headlines}").format(
        source=source, headlines=". ".join(titles)
    )


def _fetch_elpais():
    return _fetch_rss_headlines(
        "https://feeds.elpais.com/mrss-s/pages/ep/site/elpais.com/portada",
        "El País",
    )


def _fetch_bbc():
    return _fetch_rss_headlines(
        "https://feeds.bbci.co.uk/news/world/rss.xml",
        "BBC",
    )


def _fetch_catalunya():
    return _fetch_rss_headlines(
        "https://www.lavanguardia.com/rss/local/catalunya.xml",
        "La Vanguardia Catalunya",
    )


def _fetch_vilaweb():
    return _fetch_rss_headlines(
        "https://www.vilaweb.cat/feed/",
        "VilaWeb",
    )


def _format_reset_moment(value, include_date):
    """Format an ISO reset timestamp in local time, or None if unparseable."""
    try:
        moment = datetime.fromisoformat(value).astimezone()
    except (TypeError, ValueError):
        return None
    if include_date:
        return moment.strftime("%d/%m %H:%M")
    return moment.strftime("%H:%M")


def _fetch_claude_usage():
    """Read Claude Code's OAuth token and query the subscription usage limits.

    This is the same data the /usage command shows in Claude Code: the
    5-hour session window and the 7-day week window, as percentages.
    The token never leaves the machine except to query api.anthropic.com.
    """
    credentials_path = os.path.expanduser(
        os.path.join("~", ".claude", ".credentials.json")
    )
    try:
        with open(credentials_path, "r", encoding="utf-8") as credentials_file:
            token = json.load(credentials_file)["claudeAiOauth"]["accessToken"]
    except (OSError, ValueError, KeyError, TypeError):
        raise RealtimeError(
            _("Claude Code credentials not found, log in to Claude Code first.")
        )

    try:
        body = _get_json(
            "https://api.anthropic.com/api/oauth/usage",
            headers={
                "Authorization": "Bearer " + token,
                "anthropic-beta": "oauth-2025-04-20",
            },
        )
    except RealtimeError as error:
        if error.code == 401:
            raise RealtimeError(
                _("Claude Code session expired, open Claude Code to log in again.")
            )
        raise

    parts = []

    session = body.get("five_hour") or {}
    if session.get("utilization") is not None:
        percent = _format_number(session["utilization"], 0)
        reset = _format_reset_moment(session.get("resets_at"), include_date=False)
        if reset:
            parts.append(
                _("session at {percent} percent, resets at {reset}").format(
                    percent=percent, reset=reset
                )
            )
        else:
            parts.append(_("session at {percent} percent").format(percent=percent))

    week = body.get("seven_day") or {}
    if week.get("utilization") is not None:
        percent = _format_number(week["utilization"], 0)
        reset = _format_reset_moment(week.get("resets_at"), include_date=True)
        if reset:
            parts.append(
                _("week at {percent} percent, resets on {reset}").format(
                    percent=percent, reset=reset
                )
            )
        else:
            parts.append(_("week at {percent} percent").format(percent=percent))

    opus_week = body.get("seven_day_opus") or {}
    if opus_week.get("utilization") is not None:
        parts.append(
            _("opus week at {percent} percent").format(
                percent=_format_number(opus_week["utilization"], 0)
            )
        )

    if not parts:
        raise RealtimeError(
            _("The server returned data that could not be understood.")
        )

    return _("Claude usage: {parts}").format(parts=", ".join(parts))


_FETCHERS = {
    "bitcoin": _fetch_bitcoin,
    "ethereum": _fetch_ethereum,
    "eur_usd": _fetch_eur_usd,
    "brent": _fetch_brent,
    "gold": _fetch_gold,
    "ibex": _fetch_ibex,
    "weather": _fetch_weather,
    "elpais": _fetch_elpais,
    "catalunya": _fetch_catalunya,
    "vilaweb": _fetch_vilaweb,
    "bbc": _fetch_bbc,
    "claude": _fetch_claude_usage,
}
