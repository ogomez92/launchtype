// Launchtype's bridge to your Google Calendar. It runs under your own Google
// account, so no Google Cloud project is needed.
//
// 1. Paste all of this into a new project at https://script.google.com
// 2. Deploy > New deployment > Select type: Web app.
//    Execute as: Me. Who has access: Anyone.
// 3. Authorize it when Google asks (Advanced > Go to the project, since the
//    script is your own and unverified), then copy the web app URL into
//    Launchtype.
//
// "Anyone" only means the URL answers without a Google sign-in: every request
// must also carry the secret below, which Launchtype keeps in settings.json.
const TOKEN = '__TOKEN__';

// How long a new event lasts. Launchtype only ever sets the start, at 8:00.
const EVENT_MINUTES = 30;

const DAY_MS = 24 * 60 * 60 * 1000;

function doPost(e) {
  let body;
  try {
    body = handle(JSON.parse(e.postData.contents));
  } catch (error) {
    body = { error: String(error) };
  }
  return ContentService.createTextOutput(JSON.stringify(body))
    .setMimeType(ContentService.MimeType.JSON);
}

function handle(request) {
  if (request.token !== TOKEN) return { error: 'unauthorized' };
  const calendar = CalendarApp.getDefaultCalendar();
  const zone = calendar.getTimeZone();
  if (request.action === 'list') {
    const events = calendar.getEvents(new Date(request.from), new Date(request.to));
    return {
      events: events.map((event) => {
        const row = {
          id: event.getId(),
          title: event.getTitle(),
          start: event.getStartTime().toISOString(),
        };
        if (event.isAllDayEvent()) {
          row.date = Utilities.formatDate(event.getAllDayStartDate(), zone, 'yyyy-MM-dd');
        }
        return row;
      }),
    };
  }
  if (request.action === 'add') {
    const start = new Date(request.start);
    const end = new Date(start.getTime() + EVENT_MINUTES * 60 * 1000);
    calendar.createEvent(request.title, start, end);
    return { ok: true };
  }
  if (request.action === 'update') {
    // A repeating event's occurrences share an id, so the one meant is the one
    // that starts where the list said it did.
    const at = new Date(request.at);
    const event = calendar
      .getEvents(at, new Date(at.getTime() + 1000))
      .find((candidate) => candidate.getId() === request.id);
    if (!event) return { error: 'not found' };
    event.setTitle(request.title);
    if (event.isAllDayEvent()) {
      // Moved whole: an event spanning several days keeps its length.
      const days = Math.round((event.getAllDayEndDate() - event.getAllDayStartDate()) / DAY_MS);
      const start = Utilities.parseDate(request.date, zone, 'yyyy-MM-dd');
      if (days > 1) {
        event.setAllDayDates(start, new Date(start.getTime() + days * DAY_MS));
      } else {
        event.setAllDayDate(start);
      }
    } else {
      // Moved to the new day at the same time of day, keeping its length.
      const length = event.getEndTime() - event.getStartTime();
      const start = new Date(request.start);
      event.setTime(start, new Date(start.getTime() + length));
    }
    return { ok: true };
  }
  return { error: 'unknown action' };
}
