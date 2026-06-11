"""Minimal Notebrook API client.

This mirrors the HTTP calls made by the Rust `notebroocli` tool
(d:\\code\\tools\\notebroocli). Only the pieces needed to post a note are
implemented: checking the token, listing channels, creating a channel and
sending a message. The single auth mechanism is the `authorization` header
carrying the raw token (no "Bearer" prefix), exactly like the Rust client.

The standard library `urllib` is used so we don't add a new dependency.
"""

import json
import urllib.request
import urllib.error

# Network timeout in seconds for every request.
TIMEOUT = 15


class NotebrookError(Exception):
    """Raised when a Notebrook API call fails.

    ``unauthorized`` is True when the failure was an HTTP 401 so callers can
    decide to forget the stored credentials and prompt again.
    """

    def __init__(self, message, unauthorized=False):
        super().__init__(message)
        self.unauthorized = unauthorized


def _base_url(url):
    return url.rstrip("/")


def _request(method, url, token, payload=None):
    """Perform a request and return the decoded JSON body (or None).

    Maps HTTP/network failures onto :class:`NotebrookError` with a readable
    reason, matching the status handling in the Rust client.
    """
    data = None
    headers = {"authorization": token}
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["content-type"] = "application/json"

    request = urllib.request.Request(url, data=data, headers=headers, method=method)

    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
            body = response.read().decode("utf-8").strip()
            if not body:
                return None
            try:
                return json.loads(body)
            except ValueError:
                return None
    except urllib.error.HTTPError as error:
        if error.code == 401:
            raise NotebrookError(
                _("Unauthorized (401): the token was rejected."),
                unauthorized=True,
            )
        if error.code == 404:
            raise NotebrookError(
                _("Not found (404): check the server URL is correct.")
            )
        raise NotebrookError(
            _("Server returned an unexpected status code: {}").format(error.code)
        )
    except urllib.error.URLError as error:
        raise NotebrookError(
            _("Network error: {}").format(error.reason)
        )
    except Exception as error:  # noqa: BLE001 - surface anything else as a reason
        raise NotebrookError(_("Unexpected error: {}").format(error))


def check_token(url, token):
    """Validate credentials against the /check-token endpoint."""
    _request("GET", "{}/check-token".format(_base_url(url)), token)
    return True


def get_channels(url, token):
    """Return the list of channels as dicts with ``id`` and ``name``."""
    body = _request("GET", "{}/channels/".format(_base_url(url)), token)
    if not isinstance(body, dict):
        return []
    return body.get("channels", [])


def create_channel(url, token, name):
    """Create a channel and return it as a dict with ``id`` and ``name``."""
    body = _request(
        "POST", "{}/channels/".format(_base_url(url)), token, payload={"name": name}
    )
    if not isinstance(body, dict) or "id" not in body:
        raise NotebrookError(_("The server did not return the created channel."))
    return body


def send_message(url, token, channel_id, content):
    """Post a note (``content``) to the channel with the given id."""
    _request(
        "POST",
        "{}/channels/{}/messages/".format(_base_url(url), channel_id),
        token,
        payload={"content": content},
    )


def send_note(url, token, channel_name, content):
    """Send ``content`` to ``channel_name``, creating the channel if missing.

    Raises :class:`NotebrookError` with a human-readable reason on any failure.
    """
    channels = get_channels(url, token)

    channel = None
    for candidate in channels:
        if candidate.get("name") == channel_name:
            channel = candidate
            break

    if channel is None:
        channel = create_channel(url, token, channel_name)

    send_message(url, token, channel["id"], content)
