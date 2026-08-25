//! Unit conversion mode ("="): type a number, then pick what to turn it into.
//!
//! Every conversion here is a formula compiled into the app, so the mode works
//! offline and gives the same answer forever. Currencies are deliberately
//! absent for exactly that reason — an exchange rate is news, not arithmetic,
//! and "+" mode already fetches the ones worth having.
//!
//! Units are grouped into families (length, mass, shoe sizes...) and every
//! ordered pair inside a family is a row of the list, so "feet to centimeters"
//! and "centimeters to feet" are both there and neither needs a second step.
//! There are a thousand of those pairs, far too many to arrow through, so the
//! everyday ones ([`COMMON`]) lead the list and the rest are one typed word
//! away.
//!
//! Names and search words are catalog msgids like the rest of the UI: a
//! Spanish user types "pies" or "libras" and gets the same rows.

use std::sync::OnceLock;

use crate::i18n::{fold, format_args, tr, Arg};

/// How many rows the list will show at once. A bare number matches every pair
/// in the catalog, and past a few hundred rows the list is neither navigable
/// nor quick to redraw on every keystroke.
const LIST_LIMIT: usize = 300;

/// Words that join two units in a typed phrase rather than naming one
/// ("100 cm to inches"). Only tried when the phrase found nothing as it
/// stands, because "in" is also inches — dropping it up front would break
/// "5 in cm", which is the shorter way to ask the same question.
const CONNECTORS: [&str; 8] = ["to", "in", "into", "as", "the", "a", "en", "de"];

// ---------------------------------------------------------------------------
// Scales
// ---------------------------------------------------------------------------

/// How a unit relates to the base unit of its family.
#[derive(Debug, Clone, Copy)]
enum Scale {
    /// `base = value * factor + offset`. Covers almost everything, including
    /// the temperature scales, which do not share a zero with each other.
    Linear { factor: f64, offset: f64 },
    /// `base = k / value`: fuel economy, where more of one is less of the
    /// other (a car doing 40 mpg burns *fewer* liters per 100 km, not more).
    Reciprocal { k: f64 },
    /// One column of a size chart, where the base is the fractional row
    /// number. Shoe systems are related by published tables rather than by any
    /// formula, so they convert by looking up a row and reading across it.
    Chart { column: &'static [f64] },
}

/// A unit that is simply `factor` base units.
const fn times(factor: f64) -> Scale {
    Scale::Linear { factor, offset: 0.0 }
}

/// A unit whose zero is not the base unit's zero (the temperature scales).
const fn affine(factor: f64, offset: f64) -> Scale {
    Scale::Linear { factor, offset }
}

/// A unit that runs the opposite way from its base (fuel economy).
const fn inverse(k: f64) -> Scale {
    Scale::Reciprocal { k }
}

/// A column of a size chart, aligned row by row with the other columns of its
/// family.
const fn chart(column: &'static [f64]) -> Scale {
    Scale::Chart { column }
}

impl Scale {
    fn to_base(self, value: f64) -> f64 {
        match self {
            Scale::Linear { factor, offset } => value * factor + offset,
            Scale::Reciprocal { k } => k / value,
            Scale::Chart { column } => row_of(column, value),
        }
    }

    /// `to_base`'s inverse, and named after it. Clippy reads `from_` as a
    /// constructor prefix; here the pair names a direction of travel, and
    /// splitting them up would make the one call site read worse.
    #[allow(clippy::wrong_self_convention)]
    fn from_base(self, base: f64) -> f64 {
        match self {
            Scale::Linear { factor, offset } => (base - offset) / factor,
            Scale::Reciprocal { k } => k / base,
            Scale::Chart { column } => value_at(column, base),
        }
    }
}

/// The fractional row `value` sits at in `column`, interpolating between rows.
///
/// A value past either end extrapolates from the outermost pair of rows rather
/// than clamping: a foot bigger than the chart still deserves an answer, and a
/// silently clamped one would claim every giant is the same size.
fn row_of(column: &[f64], value: f64) -> f64 {
    let upper = column
        .iter()
        .position(|&row| row >= value)
        .unwrap_or(column.len() - 1)
        .clamp(1, column.len() - 1);
    let (low, high) = (column[upper - 1], column[upper]);
    (upper - 1) as f64 + (value - low) / (high - low)
}

/// The value `column` holds at fractional row `row` — the inverse of
/// [`row_of`], extrapolating past the ends the same way.
fn value_at(column: &[f64], row: f64) -> f64 {
    let upper = (row.floor() as i64 + 1).clamp(1, column.len() as i64 - 1) as usize;
    let (low, high) = (column[upper - 1], column[upper]);
    low + (row - (upper - 1) as f64) * (high - low)
}

// ---------------------------------------------------------------------------
// The catalog
// ---------------------------------------------------------------------------

/// One unit: a stable key, the msgid of its name (`"singular|plural"`, or one
/// form when the two are the same), the msgid of the extra words it can be
/// found by, the symbol a row shows for it, and how it relates to its family's
/// base.
///
/// The symbol is what the list puts in brackets after the name so you can see
/// what to type, and it is never translated: "km/h" is "km/h" everywhere. It
/// is a field of its own rather than the first search word because those two
/// are not always the same thing — the key of kilometers per hour is `kph` and
/// its symbol is `km/h`.
type Definition = (&'static str, &'static str, &'static str, &'static str, Scale);

/// A group of units that convert into one another. Nothing converts across
/// families: kilometers never become kilograms.
type Family = &'static [Definition];

/// Base: meter.
const LENGTH: Family = &[
    ("mm", "millimeter|millimeters", "mm millimetre millimetres", "mm", times(0.001)),
    ("cm", "centimeter|centimeters", "cm centimetre centimetres", "cm", times(0.01)),
    ("m", "meter|meters", "m metre metres", "m", times(1.0)),
    ("km", "kilometer|kilometers", "km kilometre kilometres", "km", times(1000.0)),
    ("in", "inch|inches", "in inches", "in", times(0.0254)),
    ("ft", "foot|feet", "ft feet", "ft", times(0.3048)),
    ("yd", "yard|yards", "yd", "yd", times(0.9144)),
    ("mi", "mile|miles", "mi", "mi", times(1609.344)),
    ("nmi", "nautical mile|nautical miles", "nmi", "nmi", times(1852.0)),
];

/// Base: kilogram.
const MASS: Family = &[
    ("mg", "milligram|milligrams", "mg milligramme", "mg", times(0.000001)),
    ("g", "gram|grams", "g gramme grammes", "g", times(0.001)),
    ("kg", "kilogram|kilograms", "kg kilo kilos kilogramme", "kg", times(1.0)),
    ("t", "metric ton|metric tons", "t tonne tonnes", "t", times(1000.0)),
    ("oz", "ounce|ounces", "oz", "oz", times(0.028349523125)),
    ("lb", "pound|pounds", "lb lbs", "lb", times(0.45359237)),
    ("st", "stone|stones", "st", "st", times(6.35029318)),
    ("ston", "short ton|short tons", "us ton", "us ton", times(907.18474)),
    ("lton", "long ton|long tons", "uk ton imperial ton", "uk ton", times(1016.0469088)),
];

/// Base: degree Celsius. The one family where the scales differ in where zero
/// falls as well as in how big a step is.
const TEMPERATURE: Family = &[
    ("c", "degree Celsius|degrees Celsius", "c celsius centigrade", "c", affine(1.0, 0.0)),
    (
        "f",
        "degree Fahrenheit|degrees Fahrenheit",
        "f fahrenheit",
        "f",
        affine(5.0 / 9.0, -160.0 / 9.0),
    ),
    ("k", "kelvin", "k", "k", affine(1.0, -273.15)),
];

/// Base: liter. The spoon-and-cup units are the US ones people cook by; the
/// imperial pint and gallon are carried separately because they are a fifth
/// bigger and the difference matters.
const VOLUME: Family = &[
    ("ml", "milliliter|milliliters", "ml millilitre cc", "ml", times(0.001)),
    ("cl", "centiliter|centiliters", "cl centilitre", "cl", times(0.01)),
    ("l", "liter|liters", "l litre litres", "l", times(1.0)),
    ("m3", "cubic meter|cubic meters", "m3 cubic metre", "m3", times(1000.0)),
    ("tsp", "teaspoon|teaspoons", "tsp", "tsp", times(0.00492892159375)),
    ("tbsp", "tablespoon|tablespoons", "tbsp", "tbsp", times(0.01478676478125)),
    ("floz", "US fluid ounce|US fluid ounces", "floz fl oz", "fl oz", times(0.0295735295625)),
    ("cup", "cup|cups", "cup", "cup", times(0.2365882365)),
    ("pt", "US pint|US pints", "pt", "pt", times(0.473176473)),
    ("qt", "US quart|US quarts", "qt", "qt", times(0.946352946)),
    ("gal", "US gallon|US gallons", "gal", "gal", times(3.785411784)),
    (
        "ukfloz",
        "imperial fluid ounce|imperial fluid ounces",
        "uk fluid ounce",
        "uk fl oz",
        times(0.0284130625),
    ),
    ("ukpt", "imperial pint|imperial pints", "uk pint", "uk pt", times(0.56826125)),
    ("ukgal", "imperial gallon|imperial gallons", "uk gallon", "uk gal", times(4.54609)),
];

/// Base: square meter.
const AREA: Family = &[
    ("mm2", "square millimeter|square millimeters", "mm2 mm²", "mm2", times(0.000001)),
    ("cm2", "square centimeter|square centimeters", "cm2 cm²", "cm2", times(0.0001)),
    ("m2", "square meter|square meters", "m2 m²", "m2", times(1.0)),
    ("km2", "square kilometer|square kilometers", "km2 km²", "km2", times(1000000.0)),
    ("ha", "hectare|hectares", "ha", "ha", times(10000.0)),
    ("acre", "acre|acres", "ac", "ac", times(4046.8564224)),
    ("ft2", "square foot|square feet", "ft2 sqft", "ft2", times(0.09290304)),
    ("yd2", "square yard|square yards", "yd2 sqyd", "yd2", times(0.83612736)),
    ("mi2", "square mile|square miles", "mi2 sqmi", "mi2", times(2589988.110336)),
];

/// Base: meter per second.
const SPEED: Family = &[
    ("mps", "meter per second|meters per second", "m/s mps", "m/s", times(1.0)),
    ("kph", "kilometer per hour|kilometers per hour", "km/h kph kmh", "km/h", times(1.0 / 3.6)),
    ("mph", "mile per hour|miles per hour", "mph", "mph", times(0.44704)),
    ("kn", "knot|knots", "kn kt", "kn", times(1852.0 / 3600.0)),
    ("fps", "foot per second|feet per second", "ft/s fps", "ft/s", times(0.3048)),
];

/// Base: pascal.
const PRESSURE: Family = &[
    ("pa", "pascal|pascals", "pa", "pa", times(1.0)),
    ("hpa", "hectopascal|hectopascals", "hpa mbar millibar", "hpa", times(100.0)),
    ("kpa", "kilopascal|kilopascals", "kpa", "kpa", times(1000.0)),
    ("bar", "bar|bars", "bar", "bar", times(100000.0)),
    ("psi", "psi", "psi pound per square inch", "psi", times(6894.757293168361)),
    ("atm", "atmosphere|atmospheres", "atm", "atm", times(101325.0)),
    (
        "mmhg",
        "millimeter of mercury|millimeters of mercury",
        "mmhg torr",
        "mmhg",
        times(133.322387415),
    ),
    ("inhg", "inch of mercury|inches of mercury", "inhg", "inhg", times(3386.388640341)),
];

/// Base: joule. The "calorie" people count on a food label is the kilocalorie,
/// so both are here and the big one says so.
const ENERGY: Family = &[
    ("j", "joule|joules", "j", "j", times(1.0)),
    ("kj", "kilojoule|kilojoules", "kj", "kj", times(1000.0)),
    ("cal", "calorie|calories", "cal", "cal", times(4.184)),
    ("kcal", "kilocalorie|kilocalories", "kcal food calorie", "kcal", times(4184.0)),
    ("wh", "watt hour|watt hours", "wh", "wh", times(3600.0)),
    ("kwh", "kilowatt hour|kilowatt hours", "kwh", "kwh", times(3600000.0)),
    ("btu", "BTU", "btu british thermal unit", "btu", times(1055.05585262)),
    ("ftlb", "foot pound|foot pounds", "ftlb", "ftlb", times(1.3558179483314004)),
];

/// Base: watt.
const POWER: Family = &[
    ("w", "watt|watts", "w", "w", times(1.0)),
    ("kw", "kilowatt|kilowatts", "kw", "kw", times(1000.0)),
    ("mgw", "megawatt|megawatts", "mw", "mw", times(1000000.0)),
    ("hp", "metric horsepower", "hp cv ps", "hp", times(735.49875)),
    ("bhp", "mechanical horsepower", "bhp", "bhp", times(745.6998715822702)),
    ("btuh", "BTU per hour", "btu/h", "btu/h", times(0.29307107017)),
];

/// Base: byte. Both the decimal units a disk is sold by and the binary ones an
/// operating system reports, which is where the missing space always went.
const DATA: Family = &[
    ("bit", "bit|bits", "bit", "bit", times(0.125)),
    ("byte", "byte|bytes", "byte", "byte", times(1.0)),
    ("kb", "kilobyte|kilobytes", "kb", "kb", times(1000.0)),
    ("mb", "megabyte|megabytes", "mb", "mb", times(1000000.0)),
    ("gb", "gigabyte|gigabytes", "gb", "gb", times(1000000000.0)),
    ("tb", "terabyte|terabytes", "tb", "tb", times(1000000000000.0)),
    ("kib", "kibibyte|kibibytes", "kib", "kib", times(1024.0)),
    ("mib", "mebibyte|mebibytes", "mib", "mib", times(1048576.0)),
    ("gib", "gibibyte|gibibytes", "gib", "gib", times(1073741824.0)),
    ("tib", "tebibyte|tebibytes", "tib", "tib", times(1099511627776.0)),
];

/// Base: second. A month is the average Gregorian one and a year is 365.2425
/// days, so "how many hours in a month" has an answer at all.
const TIME: Family = &[
    ("ms", "millisecond|milliseconds", "ms", "ms", times(0.001)),
    ("s", "second|seconds", "s sec", "s", times(1.0)),
    ("min", "minute|minutes", "min", "min", times(60.0)),
    ("h", "hour|hours", "h hr", "h", times(3600.0)),
    ("day", "day|days", "d", "d", times(86400.0)),
    ("week", "week|weeks", "wk", "wk", times(604800.0)),
    ("month", "month|months", "mo", "mo", times(2629746.0)),
    ("year", "year|years", "yr", "yr", times(31556952.0)),
];

/// Base: degree.
const ANGLE: Family = &[
    ("deg", "degree|degrees", "deg", "deg", times(1.0)),
    ("rad", "radian|radians", "rad", "rad", times(57.29577951308232)),
    ("grad", "gradian|gradians", "grad gon", "grad", times(0.9)),
    ("turn", "turn|turns", "rev revolution", "rev", times(360.0)),
];

/// Base: liters per 100 km. The one family where the numbers run in opposite
/// directions, which is exactly why nobody can do it in their head.
const FUEL: Family = &[
    (
        "l100km",
        "liter per 100 kilometers|liters per 100 kilometers",
        "l/100km",
        "l/100km",
        times(1.0),
    ),
    ("mpg", "mile per US gallon|miles per US gallon", "mpg", "mpg", inverse(235.21458333333334)),
    (
        "ukmpg",
        "mile per imperial gallon|miles per imperial gallon",
        "uk mpg",
        "uk mpg",
        inverse(282.48093627967004),
    ),
    ("kmpl", "kilometer per liter|kilometers per liter", "km/l kmpl", "km/l", inverse(100.0)),
];

/// Base: newton meter.
const TORQUE: Family = &[
    ("nm", "newton meter|newton meters", "nm", "nm", times(1.0)),
    ("lbft", "pound foot|pound feet", "lbft lb ft", "lbft", times(1.3558179483314004)),
    ("kgfm", "kilogram force meter|kilogram force meters", "kgfm kgm", "kgfm", times(9.80665)),
];

// Shoe sizes. Every column of a chart is one row of the table below, aligned
// so that reading across gives the published equivalences; the values between
// them are interpolated. Sizes that several countries share (Australia and
// India both sell in UK sizes, China and Korea both in Mondopoint millimeters)
// point at the same column, which is not a mistake — it is what the charts say.
const SHOE_MEN_CM: &[f64] =
    &[24.0, 24.5, 25.0, 25.5, 26.0, 26.5, 27.0, 27.5, 28.0, 28.5, 29.0, 29.5, 30.0, 31.0, 32.0];
const SHOE_MEN_MM: &[f64] = &[
    240.0, 245.0, 250.0, 255.0, 260.0, 265.0, 270.0, 275.0, 280.0, 285.0, 290.0, 295.0, 300.0,
    310.0, 320.0,
];
const SHOE_MEN_US: &[f64] =
    &[6.0, 6.5, 7.0, 7.5, 8.0, 8.5, 9.0, 9.5, 10.0, 10.5, 11.0, 11.5, 12.0, 13.0, 14.0];
const SHOE_MEN_UK: &[f64] =
    &[5.5, 6.0, 6.5, 7.0, 7.5, 8.0, 8.5, 9.0, 9.5, 10.0, 10.5, 11.0, 11.5, 12.5, 13.5];
const SHOE_MEN_EU: &[f64] = &[
    39.0, 39.5, 40.0, 40.5, 41.0, 42.0, 42.5, 43.0, 44.0, 44.5, 45.0, 45.5, 46.0, 47.5, 48.5,
];
const SHOE_MEN_BR: &[f64] = &[
    37.0, 37.5, 38.0, 38.5, 39.0, 40.0, 40.5, 41.0, 42.0, 42.5, 43.0, 43.5, 44.0, 45.5, 46.5,
];
const SHOE_MEN_RU: &[f64] = &[
    38.0, 38.5, 39.0, 39.5, 40.0, 41.0, 41.5, 42.0, 43.0, 43.5, 44.0, 44.5, 45.0, 46.5, 47.5,
];

const SHOE_MEN: Family = &[
    ("shoe_eu_m", "men's EU shoe size", "shoe eu europe european", "eu", chart(SHOE_MEN_EU)),
    ("shoe_uk_m", "men's UK shoe size", "shoe uk british", "uk", chart(SHOE_MEN_UK)),
    ("shoe_us_m", "men's US shoe size", "shoe us usa american", "us", chart(SHOE_MEN_US)),
    ("shoe_jp_m", "men's Japanese shoe size in cm", "shoe jp japan", "jp", chart(SHOE_MEN_CM)),
    ("shoe_mx_m", "men's Mexican shoe size in cm", "shoe mx mexico", "mx", chart(SHOE_MEN_CM)),
    ("shoe_cn_m", "men's Chinese shoe size in mm", "shoe cn china", "cn", chart(SHOE_MEN_MM)),
    ("shoe_kr_m", "men's Korean shoe size in mm", "shoe kr korea", "kr", chart(SHOE_MEN_MM)),
    ("shoe_br_m", "men's Brazilian shoe size", "shoe br brazil", "br", chart(SHOE_MEN_BR)),
    ("shoe_ru_m", "men's Russian shoe size", "shoe ru russia", "ru", chart(SHOE_MEN_RU)),
    ("shoe_au_m", "men's Australian shoe size", "shoe au australia", "au", chart(SHOE_MEN_UK)),
    ("shoe_in_m", "men's Indian shoe size", "shoe india", "india", chart(SHOE_MEN_UK)),
];

const SHOE_WOMEN_CM: &[f64] =
    &[21.5, 22.0, 22.5, 23.0, 23.5, 24.0, 24.5, 25.0, 25.5, 26.0, 26.5, 27.0, 27.5, 28.0];
const SHOE_WOMEN_MM: &[f64] = &[
    215.0, 220.0, 225.0, 230.0, 235.0, 240.0, 245.0, 250.0, 255.0, 260.0, 265.0, 270.0, 275.0,
    280.0,
];
const SHOE_WOMEN_US: &[f64] =
    &[5.0, 5.5, 6.0, 6.5, 7.0, 7.5, 8.0, 8.5, 9.0, 9.5, 10.0, 10.5, 11.0, 11.5];
const SHOE_WOMEN_UK: &[f64] =
    &[2.5, 3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 6.0, 6.5, 7.0, 7.5, 8.0, 8.5, 9.0];
const SHOE_WOMEN_EU: &[f64] =
    &[35.0, 35.5, 36.0, 37.0, 37.5, 38.0, 38.5, 39.0, 40.0, 40.5, 41.0, 42.0, 42.5, 43.0];
const SHOE_WOMEN_BR: &[f64] =
    &[33.0, 33.5, 34.0, 35.0, 35.5, 36.0, 36.5, 37.0, 38.0, 38.5, 39.0, 40.0, 40.5, 41.0];
const SHOE_WOMEN_RU: &[f64] =
    &[34.0, 34.5, 35.0, 36.0, 36.5, 37.0, 37.5, 38.0, 39.0, 39.5, 40.0, 41.0, 41.5, 42.0];

const SHOE_WOMEN: Family = &[
    ("shoe_eu_w", "women's EU shoe size", "shoe eu europe european", "eu", chart(SHOE_WOMEN_EU)),
    ("shoe_uk_w", "women's UK shoe size", "shoe uk british", "uk", chart(SHOE_WOMEN_UK)),
    ("shoe_us_w", "women's US shoe size", "shoe us usa american", "us", chart(SHOE_WOMEN_US)),
    ("shoe_jp_w", "women's Japanese shoe size in cm", "shoe jp japan", "jp", chart(SHOE_WOMEN_CM)),
    ("shoe_mx_w", "women's Mexican shoe size in cm", "shoe mx mexico", "mx", chart(SHOE_WOMEN_CM)),
    ("shoe_cn_w", "women's Chinese shoe size in mm", "shoe cn china", "cn", chart(SHOE_WOMEN_MM)),
    ("shoe_kr_w", "women's Korean shoe size in mm", "shoe kr korea", "kr", chart(SHOE_WOMEN_MM)),
    ("shoe_br_w", "women's Brazilian shoe size", "shoe br brazil", "br", chart(SHOE_WOMEN_BR)),
    ("shoe_ru_w", "women's Russian shoe size", "shoe ru russia", "ru", chart(SHOE_WOMEN_RU)),
    ("shoe_au_w", "women's Australian shoe size", "shoe au australia", "au", chart(SHOE_WOMEN_UK)),
    ("shoe_in_w", "women's Indian shoe size", "shoe india", "india", chart(SHOE_WOMEN_UK)),
];

const FAMILIES: &[Family] = &[
    LENGTH,
    MASS,
    TEMPERATURE,
    VOLUME,
    AREA,
    SPEED,
    PRESSURE,
    ENERGY,
    POWER,
    DATA,
    TIME,
    ANGLE,
    FUEL,
    TORQUE,
    SHOE_MEN,
    SHOE_WOMEN,
];

/// The conversions worth arrowing to before any word is typed, best first.
/// Everything else in the catalog follows them in family order.
const COMMON: &[(&str, &str)] = &[
    ("c", "f"),
    ("f", "c"),
    ("cm", "in"),
    ("in", "cm"),
    ("ft", "cm"),
    ("cm", "ft"),
    ("ft", "m"),
    ("m", "ft"),
    ("km", "mi"),
    ("mi", "km"),
    ("kg", "lb"),
    ("lb", "kg"),
    ("g", "oz"),
    ("oz", "g"),
    ("st", "kg"),
    ("kg", "st"),
    ("mm", "in"),
    ("in", "mm"),
    ("m", "yd"),
    ("yd", "m"),
    ("kph", "mph"),
    ("mph", "kph"),
    ("l", "gal"),
    ("gal", "l"),
    ("l", "ukgal"),
    ("ukgal", "l"),
    ("ml", "floz"),
    ("floz", "ml"),
    ("ml", "cup"),
    ("cup", "ml"),
    ("tbsp", "ml"),
    ("tsp", "ml"),
    ("m2", "ft2"),
    ("ft2", "m2"),
    ("ha", "acre"),
    ("acre", "ha"),
    ("kw", "hp"),
    ("hp", "kw"),
    ("kcal", "kj"),
    ("kj", "kcal"),
    ("bar", "psi"),
    ("psi", "bar"),
    ("gb", "gib"),
    ("gib", "gb"),
    ("mb", "mib"),
    ("tb", "tib"),
    ("h", "min"),
    ("min", "h"),
    ("day", "h"),
    ("year", "day"),
    ("l100km", "mpg"),
    ("mpg", "l100km"),
    ("mi", "nmi"),
    ("nmi", "mi"),
    ("deg", "rad"),
    ("rad", "deg"),
    ("nm", "lbft"),
    ("lbft", "nm"),
    ("shoe_eu_m", "shoe_us_m"),
    ("shoe_us_m", "shoe_eu_m"),
    ("shoe_eu_m", "shoe_uk_m"),
    ("shoe_uk_m", "shoe_us_m"),
    ("shoe_eu_w", "shoe_us_w"),
    ("shoe_us_w", "shoe_eu_w"),
    ("shoe_eu_w", "shoe_uk_w"),
    ("shoe_uk_w", "shoe_us_w"),
];

// ---------------------------------------------------------------------------
// The catalog, resolved
// ---------------------------------------------------------------------------

/// A unit, its family and its localized text, resolved once from [`FAMILIES`].
struct Entry {
    key: &'static str,
    /// Index into [`FAMILIES`]; only units of the same family convert.
    family: usize,
    scale: Scale,
    /// The name for a value of one, and for any other value ("1 foot",
    /// "3 feet"). Both are the same string for units whose name does not
    /// inflect ("men's UK shoe size", "psi").
    singular: String,
    plural: String,
    /// The symbol a row shows in brackets after the name, so that what to type
    /// is on screen next to what it means. Empty when the name already says it
    /// ("men's EU shoe size" needs no "(eu)" after it), which is the only
    /// reason to leave it out — otherwise every row carries one.
    symbol: &'static str,
    /// Every word this unit can be found by: its names, its search msgid and
    /// its symbol, folded and stripped of punctuation. The symbol is in here
    /// because a row that shows one is promising it can be typed.
    keys: Vec<String>,
}

/// The whole catalog, localized. Built on first use: the translation catalog is
/// installed at startup and never changes afterwards, exactly as the emoji
/// table assumes.
fn entries() -> &'static [Entry] {
    static ENTRIES: OnceLock<Vec<Entry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let mut entries = Vec::new();
        for (family, units) in FAMILIES.iter().enumerate() {
            for &(key, name, aliases, symbol, scale) in *units {
                let name = tr(name);
                let (singular, plural) = match name.split_once('|') {
                    Some((one, many)) => (one.to_string(), many.to_string()),
                    None => (name.clone(), name.clone()),
                };
                let mut keys = words(&singular);
                let extra = words(&plural).into_iter().chain(words(&tr(aliases)));
                for word in extra.chain(words(symbol)) {
                    // A symbol with a slash in it is as likely to be typed
                    // without one, and "kmh" finding nothing while "km/h"
                    // works is the kind of dead end nobody would guess at.
                    let bare = word.replace('/', "");
                    for word in [word, bare] {
                        if !keys.contains(&word) {
                            keys.push(word);
                        }
                    }
                }
                // A symbol the name already spells out would only be read
                // twice over: "42 men's EU shoe size (eu)", "2 psi (psi)".
                let spelled_out: Vec<String> =
                    words(&singular).into_iter().chain(words(&plural)).collect();
                let redundant = words(symbol).iter().all(|word| spelled_out.contains(word));
                let symbol = if redundant { "" } else { symbol };
                entries.push(Entry { key, family, scale, singular, plural, symbol, keys });
            }
        }
        entries
    })
}

/// The searchable words of a phrase: folded, and stripped of the punctuation
/// nobody types ("men's" becomes "mens", "(cm)" becomes "cm"). Slashes survive,
/// because "km/h" is one word and not two.
fn words(text: &str) -> Vec<String> {
    fold(text)
        .split_whitespace()
        .map(|word| word.chars().filter(|c| c.is_alphanumeric() || *c == '/').collect::<String>())
        .filter(|word| !word.is_empty())
        .collect()
}

/// One row of the "=" list: a conversion in one direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Conversion {
    from: usize,
    to: usize,
}

/// Every ordered pair of units inside a family, the everyday ones first.
fn conversions() -> &'static [Conversion] {
    static PAIRS: OnceLock<Vec<Conversion>> = OnceLock::new();
    PAIRS.get_or_init(|| {
        let entries = entries();
        let mut pairs: Vec<Conversion> = Vec::new();
        for (from, from_entry) in entries.iter().enumerate() {
            for (to, to_entry) in entries.iter().enumerate() {
                if from != to && from_entry.family == to_entry.family {
                    pairs.push(Conversion { from, to });
                }
            }
        }
        // A stable sort, so pairs that are equally common keep catalog order.
        pairs.sort_by_key(|pair| {
            let (from, to) = (entries[pair.from].key, entries[pair.to].key);
            COMMON
                .iter()
                .position(|&common| common == (from, to))
                .unwrap_or(usize::MAX)
        });
        pairs
    })
}

fn entry(key: &str) -> Option<&'static Entry> {
    entries().iter().find(|entry| entry.key == key)
}

/// `value` expressed in `to_key`, or `None` if either unit is unknown or the
/// two measure different things.
pub fn convert(from_key: &str, to_key: &str, value: f64) -> Option<f64> {
    let (from, to) = (entry(from_key)?, entry(to_key)?);
    if from.family != to.family {
        return None;
    }
    Some(to.scale.from_base(from.scale.to_base(value)))
}

// ---------------------------------------------------------------------------
// Reading what was typed
// ---------------------------------------------------------------------------

/// Split typed text into the number to convert and the words that pick the
/// conversion. "100ft cm" and "100 ft cm" both give `(100, "ft cm")`; a comma
/// is a decimal point, because half the world types it that way.
pub fn parse_input(text: &str) -> (Option<f64>, &str) {
    let text = text.trim_start();
    let mut end = 0;
    for (index, c) in text.char_indices() {
        let numeric = c.is_ascii_digit()
            || c == '.'
            || c == ','
            || ((c == '-' || c == '+') && index == 0);
        if !numeric {
            break;
        }
        end = index + c.len_utf8();
    }
    match text[..end].replace(',', ".").parse::<f64>() {
        Ok(value) if value.is_finite() => (Some(value), text[end..].trim_start()),
        _ => (None, text),
    }
}

/// A number as the list should read it: enough decimals to stay exact where it
/// matters, and none of the trailing zeros that make a screen reader drone.
pub fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs();
    let decimals = if magnitude >= 1.0 {
        4
    } else {
        // Four significant digits for a small number: 0.0000254, not 0.0000.
        ((-magnitude.log10().floor()) as usize + 3).min(12)
    };
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    };
    // A number smaller than the decimals allowed for would print as plain "0",
    // which is not an answer; say it the only way that fits instead.
    if text == "0" || text == "-0" {
        format!("{value:e}")
    } else {
        text
    }
}

/// How well a pair answers `tokens`, as a tier where lower is better, or
/// `None` if it does not answer them at all.
///
/// The tiers, in order: the words name the source unit and then the target
/// ("ft cm"), then the source alone, then the target alone, then — as a last
/// resort — the words name the two sides in some other order ("cm ft" when
/// what is meant is feet to centimeters, which is a row further down).
fn rank(from: &Entry, to: &Entry, tokens: &[String]) -> Option<u8> {
    let mut best: Option<u8> = None;
    for split in 1..tokens.len() {
        let (head, tail) = tokens.split_at(split);
        if let (Some(a), Some(b)) = (matches_all(from, head), matches_all(to, tail)) {
            let tier = a.max(b);
            best = Some(best.map_or(tier, |previous: u8| previous.min(tier)));
        }
    }
    if let Some(tier) = best {
        return Some(tier);
    }
    if let Some(tier) = matches_all(from, tokens) {
        return Some(3 + tier);
    }
    if let Some(tier) = matches_all(to, tokens) {
        return Some(6 + tier);
    }
    tokens
        .iter()
        .all(|token| word_tier(from, token).is_some() || word_tier(to, token).is_some())
        .then_some(9)
}

/// The weakest tier among `tokens` when every one of them names `unit`, or
/// `None` as soon as one of them does not.
fn matches_all(unit: &Entry, tokens: &[String]) -> Option<u8> {
    let mut weakest = 0;
    for token in tokens {
        weakest = weakest.max(word_tier(unit, token)?);
    }
    Some(weakest)
}

/// 0 when `word` is one of the unit's words, 1 when it starts one, 2 when it
/// is inside one, `None` when it is nowhere in it.
///
/// Only a word of three letters or more is looked for *inside* another. A
/// symbol is one or two letters and turns up inside half the catalog by
/// coincidence — "l" is in gallon, calorie and kelvin alike — which left
/// typing a liter matching three hundred rows that had nothing to do with it.
fn word_tier(unit: &Entry, word: &str) -> Option<u8> {
    if unit.keys.iter().any(|key| key == word) {
        return Some(0);
    }
    if unit.keys.iter().any(|key| key.starts_with(word)) {
        return Some(1);
    }
    if word.chars().count() > 2 && unit.keys.iter().any(|key| key.contains(word)) {
        return Some(2);
    }
    None
}

/// The conversions `query` asks for, best first. An empty query is the whole
/// catalog, everyday conversions first.
///
/// A joining word only joins when it stands between two units: the "in" of
/// "cm in inches" is one, the "in" of "5 in cm" is inches. So the phrase is
/// tried as typed first, then without the joining words in the middle of it,
/// and only then without any of them at all.
fn search(query: &str) -> Vec<Conversion> {
    let tokens = words(query);
    if tokens.is_empty() {
        return conversions().to_vec();
    }
    let mut attempts = vec![tokens.clone()];
    for candidate in [without_connectors(&tokens, false), without_connectors(&tokens, true)] {
        if !attempts.contains(&candidate) {
            attempts.push(candidate);
        }
    }
    for candidate in &attempts {
        // Nothing but joining words: no unit was asked for at all.
        if candidate.is_empty() {
            return conversions().to_vec();
        }
        let hits = ranked(candidate);
        if !hits.is_empty() {
            return hits;
        }
    }
    Vec::new()
}

/// The phrase without its joining words: all of them when `every`, and
/// otherwise only those standing between two other words.
fn without_connectors(tokens: &[String], every: bool) -> Vec<String> {
    tokens
        .iter()
        .enumerate()
        .filter(|&(index, token)| {
            let joining = every || (index > 0 && index + 1 < tokens.len());
            !(joining && CONNECTORS.contains(&token.as_str()))
        })
        .map(|(_, token)| token.clone())
        .collect()
}

fn ranked(tokens: &[String]) -> Vec<Conversion> {
    let entries = entries();
    let mut hits: Vec<(u8, Conversion)> = conversions()
        .iter()
        .filter_map(|&pair| {
            rank(&entries[pair.from], &entries[pair.to], tokens).map(|tier| (tier, pair))
        })
        .collect();
    // Stable, so pairs on the same tier keep the everyday-first catalog order.
    hits.sort_by_key(|&(tier, _)| tier);
    hits.into_iter().map(|(_, pair)| pair).collect()
}

// ---------------------------------------------------------------------------
// Rows for the UI
// ---------------------------------------------------------------------------

/// One row of the "=" list.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// What the list shows and the screen reader announces.
    pub label: String,
    /// The converted number on its own, ready for the clipboard. `None` until
    /// a number is typed, and for the conversions that have no answer for it
    /// (nothing does 0 miles per gallon).
    pub result: Option<String>,
    /// `from>to`, stable across keystrokes.
    pub id: String,
}

/// The rows to show for everything typed after the "=" that entered the mode.
pub fn rows(input: &str) -> Vec<Row> {
    let (value, query) = parse_input(input);
    let entries = entries();
    search(query)
        .into_iter()
        .take(LIST_LIMIT)
        .map(|pair| {
            let (from, to) = (&entries[pair.from], &entries[pair.to]);
            let result = value
                .map(|value| to.scale.from_base(from.scale.to_base(value)))
                .filter(|result| result.is_finite());
            let label = match (value, result) {
                (Some(value), Some(result)) => format_args(
                    &tr("{value} {from} = {result} {to}"),
                    &[
                        ("value", Arg::Str(&format_number(value))),
                        ("from", Arg::Str(&name(from, value == 1.0))),
                        ("result", Arg::Str(&format_number(result))),
                        ("to", Arg::Str(&name(to, result == 1.0))),
                    ],
                ),
                _ => format_args(
                    &tr("{from} to {to}"),
                    &[
                        ("from", Arg::Str(&name(from, false))),
                        ("to", Arg::Str(&name(to, false))),
                    ],
                ),
            };
            Row { label, result: result.map(format_number), id: format!("{}>{}", from.key, to.key) }
        })
        .collect()
}

/// How a row names a unit: "3 feet (ft)", but "1 foot (ft)".
///
/// The symbol rides along in brackets so that the thing to type is on screen
/// next to the thing it means — you find out that centimeters answer to "cm"
/// by reading a row, not by reading the manual.
fn name(unit: &Entry, singular: bool) -> String {
    let name = if singular { &unit.singular } else { &unit.plural };
    if unit.symbol.is_empty() {
        name.clone()
    } else {
        format!("{name} ({})", unit.symbol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `value` converted, rounded to `places` so a test can state the answer
    /// the way a conversion table does.
    fn round(value: f64, places: i32) -> f64 {
        let scale = 10f64.powi(places);
        (value * scale).round() / scale
    }

    fn to(from: &str, to: &str, value: f64) -> f64 {
        convert(from, to, value).unwrap_or_else(|| panic!("{from} to {to} did not convert"))
    }

    fn labels(input: &str) -> Vec<String> {
        rows(input).into_iter().map(|row| row.label).collect()
    }

    #[test]
    fn the_catalog_is_well_formed() {
        let entries = entries();
        assert!(entries.len() > 100, "only {} units", entries.len());
        for entry in entries {
            assert!(!entry.singular.is_empty(), "{} has no name", entry.key);
            assert!(!entry.keys.is_empty(), "{} has no search words", entry.key);
            assert_eq!(entries.iter().filter(|e| e.key == entry.key).count(), 1, "{}", entry.key);
        }
    }

    /// Every column of a size chart has to line up with the others row for row,
    /// or reading across gives a size from the wrong line.
    #[test]
    fn size_charts_line_up_and_climb() {
        for family in FAMILIES {
            let columns: Vec<&[f64]> = family
                .iter()
                .filter_map(|&(_, _, _, _, scale)| match scale {
                    Scale::Chart { column } => Some(column),
                    _ => None,
                })
                .collect();
            for column in &columns {
                assert!(column.len() >= 2, "a chart column needs two rows to interpolate");
                assert_eq!(column.len(), columns[0].len(), "chart columns are ragged");
                assert!(
                    column.windows(2).all(|pair| pair[1] > pair[0]),
                    "a chart column has to climb: {column:?}"
                );
            }
        }
    }

    #[test]
    fn every_common_pair_is_a_real_conversion() {
        for &(from, to) in COMMON {
            assert_ne!(from, to, "{from} converts to itself");
            assert!(convert(from, to, 1.0).is_some(), "{from} to {to} is not in the catalog");
        }
    }

    /// The list is generated, so a family with a unit in it that nothing can
    /// reach would go unnoticed.
    #[test]
    fn every_unit_pairs_with_the_rest_of_its_family() {
        let entries = entries();
        for (index, entry) in entries.iter().enumerate() {
            let siblings = entries.iter().filter(|other| other.family == entry.family).count();
            let pairs = conversions().iter().filter(|pair| pair.from == index).count();
            assert_eq!(pairs, siblings - 1, "{} pairs with {pairs} units", entry.key);
        }
    }

    #[test]
    fn length_matches_the_definitions() {
        assert_eq!(to("ft", "cm", 1.0), 30.48);
        assert_eq!(to("in", "cm", 1.0), 2.54);
        assert_eq!(round(to("mi", "km", 1.0), 6), 1.609344);
        assert_eq!(round(to("km", "mi", 100.0), 4), 62.1371);
        assert_eq!(round(to("cm", "ft", 180.0), 4), 5.9055);
        assert_eq!(to("nmi", "m", 1.0), 1852.0);
    }

    #[test]
    fn mass_matches_the_definitions() {
        assert_eq!(round(to("kg", "lb", 1.0), 5), 2.20462);
        assert_eq!(round(to("lb", "kg", 150.0), 4), 68.0389);
        assert_eq!(round(to("oz", "g", 1.0), 6), 28.349523);
        assert_eq!(to("st", "lb", 1.0).round(), 14.0);
        assert_eq!(to("t", "kg", 1.0), 1000.0);
    }

    /// The only family whose scales disagree about where zero is, and the one
    /// place a plain multiplication would be wrong in both directions.
    #[test]
    fn temperature_crosses_the_offset_both_ways() {
        assert_eq!(round(to("c", "f", 0.0), 6), 32.0);
        assert_eq!(round(to("c", "f", 100.0), 6), 212.0);
        assert_eq!(round(to("f", "c", 98.6), 4), 37.0);
        assert_eq!(round(to("c", "k", 0.0), 6), 273.15);
        assert_eq!(round(to("k", "c", 0.0), 6), -273.15);
        // The one temperature both scales agree on.
        assert_eq!(round(to("c", "f", -40.0), 6), -40.0);
        assert_eq!(round(to("f", "c", -40.0), 6), -40.0);
    }

    #[test]
    fn volume_and_area_match_the_definitions() {
        assert_eq!(round(to("gal", "l", 1.0), 6), 3.785412);
        assert_eq!(round(to("ukgal", "l", 1.0), 5), 4.54609);
        assert_eq!(round(to("cup", "ml", 1.0), 4), 236.5882);
        assert_eq!(round(to("l", "gal", 10.0), 4), 2.6417);
        assert_eq!(round(to("acre", "m2", 1.0), 4), 4046.8564);
        assert_eq!(round(to("ha", "acre", 1.0), 4), 2.4711);
        assert_eq!(round(to("m2", "ft2", 100.0), 3), 1076.391);
    }

    #[test]
    fn speed_pressure_energy_and_power_match_the_definitions() {
        assert_eq!(round(to("kph", "mph", 100.0), 4), 62.1371);
        assert_eq!(round(to("kn", "kph", 1.0), 4), 1.852);
        assert_eq!(round(to("bar", "psi", 1.0), 4), 14.5038);
        assert_eq!(round(to("atm", "hpa", 1.0), 2), 1013.25);
        assert_eq!(round(to("kcal", "kj", 1.0), 4), 4.184);
        assert_eq!(round(to("kwh", "j", 1.0), 0), 3600000.0);
        assert_eq!(round(to("hp", "kw", 1.0), 5), 0.73550);
        assert_eq!(round(to("kw", "bhp", 100.0), 3), 134.102);
    }

    #[test]
    fn data_keeps_the_decimal_and_binary_units_apart() {
        assert_eq!(to("gb", "byte", 1.0), 1e9);
        assert_eq!(to("gib", "byte", 1.0), 1073741824.0);
        // The gigabyte a disk is sold by, as the operating system reports it.
        assert_eq!(round(to("gb", "gib", 500.0), 3), 465.661);
        assert_eq!(to("byte", "bit", 1.0), 8.0);
    }

    #[test]
    fn time_and_angle_match_the_definitions() {
        assert_eq!(to("h", "min", 1.5), 90.0);
        assert_eq!(to("day", "h", 1.0), 24.0);
        assert_eq!(round(to("year", "day", 1.0), 4), 365.2425);
        assert_eq!(round(to("rad", "deg", 1.0), 4), 57.2958);
        assert_eq!(round(to("deg", "rad", 180.0), 6), round(std::f64::consts::PI, 6));
        assert_eq!(to("turn", "deg", 1.0), 360.0);
    }

    /// Fuel economy is the family that runs backwards: a bigger mpg is a
    /// smaller l/100km, and a multiplication would get the direction wrong.
    #[test]
    fn fuel_economy_runs_the_other_way() {
        assert_eq!(round(to("mpg", "l100km", 30.0), 3), 7.84);
        assert_eq!(round(to("l100km", "mpg", 7.84), 2), 30.0);
        assert_eq!(round(to("ukmpg", "l100km", 40.0), 3), 7.062);
        assert_eq!(round(to("kmpl", "l100km", 20.0), 3), 5.0);
        // Bigger mpg, smaller consumption.
        assert!(to("mpg", "l100km", 50.0) < to("mpg", "l100km", 25.0));
    }

    /// The published men's chart, read across the row people know best.
    #[test]
    fn shoe_sizes_read_across_the_chart() {
        assert_eq!(to("shoe_us_m", "shoe_eu_m", 9.0), 42.5);
        assert_eq!(to("shoe_us_m", "shoe_uk_m", 9.0), 8.5);
        assert_eq!(to("shoe_us_m", "shoe_jp_m", 9.0), 27.0);
        assert_eq!(to("shoe_eu_m", "shoe_us_m", 44.0), 10.0);
        assert_eq!(to("shoe_us_w", "shoe_eu_w", 8.0), 38.5);
        assert_eq!(to("shoe_eu_w", "shoe_uk_w", 39.0), 6.0);
        assert_eq!(to("shoe_us_w", "shoe_cn_w", 7.0), 235.0);
        // Australia and India sell in UK sizes, China and Korea in millimeters.
        assert_eq!(to("shoe_uk_m", "shoe_au_m", 9.0), 9.0);
        assert_eq!(to("shoe_kr_m", "shoe_cn_m", 270.0), 270.0);
    }

    /// A size between two printed rows, and one past the end of the chart:
    /// both get an answer rather than the nearest row twice over.
    #[test]
    fn shoe_sizes_interpolate_and_extrapolate() {
        let between = to("shoe_jp_m", "shoe_us_m", 26.75);
        assert!((8.5..8.9).contains(&between), "{between}");
        assert!(to("shoe_us_m", "shoe_eu_m", 16.0) > 48.5);
        assert!(to("shoe_us_m", "shoe_eu_m", 4.0) < 39.0);
    }

    #[test]
    fn every_unit_survives_a_round_trip() {
        for from in entries() {
            for to in entries().iter().filter(|to| to.family == from.family) {
                let there = convert(from.key, to.key, 7.0).unwrap();
                let back = convert(to.key, from.key, there).unwrap();
                assert!(
                    (back - 7.0).abs() < 1e-6,
                    "{} to {} and back gave {back}",
                    from.key,
                    to.key
                );
            }
        }
    }

    #[test]
    fn unrelated_units_do_not_convert() {
        assert_eq!(convert("km", "kg", 1.0), None);
        assert_eq!(convert("c", "ft", 1.0), None);
        assert_eq!(convert("shoe_eu_m", "shoe_eu_w", 42.0), None);
        assert_eq!(convert("nope", "km", 1.0), None);
    }

    #[test]
    fn numbers_are_read_off_the_front() {
        assert_eq!(parse_input("100 ft cm"), (Some(100.0), "ft cm"));
        assert_eq!(parse_input("100ft cm"), (Some(100.0), "ft cm"));
        assert_eq!(parse_input("100"), (Some(100.0), ""));
        assert_eq!(parse_input("-40 c f"), (Some(-40.0), "c f"));
        assert_eq!(parse_input("5.5 shoe"), (Some(5.5), "shoe"));
        // Half the world types the decimal point as a comma.
        assert_eq!(parse_input("1,5 kg lb"), (Some(1.5), "kg lb"));
        assert_eq!(parse_input("ft cm"), (None, "ft cm"));
        assert_eq!(parse_input(""), (None, ""));
        assert_eq!(parse_input("-"), (None, "-"));
    }

    #[test]
    fn numbers_are_written_without_trailing_noise() {
        assert_eq!(format_number(30.48), "30.48");
        assert_eq!(format_number(3048.0), "3048");
        assert_eq!(format_number(212.00000000000003), "212");
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(-40.0), "-40");
        assert_eq!(format_number(1.0 / 3.0), "0.3333");
        assert_eq!(format_number(0.0000254), "0.0000254");
        assert_eq!(format_number(f64::INFINITY), "");
        // Too small for any sensible number of decimals: say it the other way
        // rather than claim it is zero.
        assert_eq!(format_number(1e-20), "1e-20");
    }

    /// The heart of the mode: a number and nothing else still answers.
    #[test]
    fn a_bare_number_lists_the_everyday_conversions() {
        let rows = rows("100");
        assert_eq!(rows.len(), LIST_LIMIT);
        assert_eq!(rows[0].label, "100 degrees Celsius (c) = 212 degrees Fahrenheit (f)");
        assert_eq!(rows[0].result.as_deref(), Some("212"));
        assert_eq!(rows[0].id, "c>f");
        assert!(rows.iter().any(|row| row.label == "100 feet (ft) = 3048 centimeters (cm)"));
        assert!(rows.iter().any(|row| row.label == "100 kilograms (kg) = 220.4623 pounds (lb)"));
    }

    #[test]
    fn a_unit_of_one_is_named_in_the_singular() {
        assert_eq!(rows("1 ft m")[0].label, "1 foot (ft) = 0.3048 meters (m)");
        assert_eq!(rows("3 ft m")[0].label, "3 feet (ft) = 0.9144 meters (m)");
    }

    /// Each row says what to type for the units it names, which is the only
    /// place the symbols are ever advertised — except where the name already
    /// spells the symbol out, and repeating it would just be read twice.
    #[test]
    fn rows_show_the_symbol_to_type() {
        let speed = "10 miles per hour (mph) = 16.0934 kilometers per hour (km/h)";
        assert_eq!(rows("10 mph kph")[0].label, speed);
        assert_eq!(rows("1 gb mb")[0].label, "1 gigabyte (gb) = 1000 megabytes (mb)");
        assert_eq!(rows("1 l gal")[0].label, "1 liter (l) = 0.2642 US gallons (gal)");
        // "psi", "bars" and "men's EU shoe size" already say their own symbol.
        assert_eq!(rows("1 psi bar")[0].label, "1 psi = 0.06895 bars");
        assert!(labels("42 eu us shoe men")[0].ends_with("men's US shoe size"));
    }

    /// Anything a row offers as the thing to type has to actually find it —
    /// slash and all, and equally without the slash, which is how a symbol
    /// like "km/h" usually gets typed.
    #[test]
    fn every_symbol_a_row_shows_can_be_typed() {
        for entry in entries().iter().filter(|entry| !entry.symbol.is_empty()) {
            for typed in [entry.symbol.to_string(), entry.symbol.replace('/', "")] {
                let found = rows(&format!("1 {typed}"));
                assert!(
                    found.iter().any(|row| row.id.starts_with(&format!("{}>", entry.key))),
                    "typing {typed:?} finds no {} row",
                    entry.key
                );
            }
        }
    }

    /// With no number at all the list is still browsable, so the mode can be
    /// explored before anything is typed.
    #[test]
    fn without_a_number_the_rows_name_the_pair() {
        let browsing = rows("");
        assert_eq!(browsing[0].label, "degrees Celsius (c) to degrees Fahrenheit (f)");
        assert_eq!(browsing[0].result, None);
        assert_eq!(rows("ft cm")[0].label, "feet (ft) to centimeters (cm)");
    }

    #[test]
    fn the_words_after_the_number_pick_the_conversion() {
        assert_eq!(labels("100 ft cm")[0], "100 feet (ft) = 3048 centimeters (cm)");
        assert_eq!(labels("100 cm ft")[0], "100 centimeters (cm) = 3.2808 feet (ft)");
        assert_eq!(labels("30 c f")[0], "30 degrees Celsius (c) = 86 degrees Fahrenheit (f)");
        assert_eq!(labels("70 kg lb")[0], "70 kilograms (kg) = 154.3236 pounds (lb)");
        assert_eq!(labels("5 miles kilometers")[0], "5 miles (mi) = 8.0467 kilometers (km)");
    }

    /// "in" is inches, so joining words can only be dropped once the phrase has
    /// failed as it stands — both of these have to work.
    #[test]
    fn joining_words_are_forgiven_but_inches_still_win() {
        let cm_to_in = "100 centimeters (cm) = 39.3701 inches (in)";
        let in_to_cm = "5 inches (in) = 12.7 centimeters (cm)";
        assert_eq!(labels("100 cm to inches")[0], cm_to_in);
        assert_eq!(labels("100 cm in inches")[0], cm_to_in);
        assert_eq!(labels("5 in cm")[0], in_to_cm);
        assert_eq!(labels("5 in to cm")[0], in_to_cm);
    }

    /// Naming one unit lists it as the thing being converted first, and as the
    /// thing being converted *to* further down — "2 kg" is as likely to mean
    /// "what is 2 kg in pounds" as "what is 2 of something in kg".
    #[test]
    fn one_unit_lists_it_in_both_directions() {
        let labels = labels("2 kg");
        assert!(labels[0].starts_with("2 kilograms (kg) = "), "{}", labels[0]);
        assert!(labels
            .iter()
            .any(|label| label.ends_with(" kilograms (kg)") && !label.starts_with("2 kilograms")));
    }

    #[test]
    fn shoe_sizes_are_found_by_country() {
        let eu_to_us = "42 men's EU shoe size = 8.5 men's US shoe size";
        assert_eq!(labels("42 eu us shoe men")[0], eu_to_us);
        assert!(labels("39 shoe")
            .iter()
            .any(|label| label.contains("Japanese") || label.contains("japanese")));
        assert!(labels("8 shoe women brazil").iter().any(|label| label.contains("Brazilian")));
    }

    /// A symbol is one or two letters and hides inside half the catalog by
    /// coincidence, so it is only ever matched at the start of a word. Before
    /// that rule "2 l" filtered nothing: it matched every calorie, gallon and
    /// kelvin in the table and filled the list to its limit.
    #[test]
    fn a_symbol_matches_where_a_word_starts_and_not_in_the_middle_of_one() {
        let liters = labels("2 l");
        assert!(liters[0].starts_with("2 liters (l) = "), "{}", liters[0]);
        assert!(liters.len() < LIST_LIMIT / 2, "{} rows for a liter", liters.len());
        // Each of these hides an "l" in the middle of its name, has no symbol
        // that starts with one, and belongs to a family with nothing else that
        // does — so no row of theirs can be what was asked for. Gallons and
        // pounds are no good as examples here: a gallon converts to liters and
        // a pound is an "lb", which makes both of those legitimate answers.
        for unit in ["calories", "kelvin", "miles (mi) =", "square miles"] {
            let spurious = format!("2 {unit}");
            assert!(
                !liters.iter().any(|label| label.starts_with(&spurious)),
                "{unit} matched \"l\""
            );
        }
        // Three letters and up still match inside a word, which is what makes
        // "gallon" reach "US gallon" and "imperial gallon" alike.
        assert!(labels("1 gallon").iter().filter(|label| label.contains("gallon")).count() >= 2);
    }

    #[test]
    fn a_word_that_names_nothing_lists_nothing() {
        assert!(rows("100 zzzz").is_empty());
        assert!(rows("100 ft zzzz").is_empty());
    }

    /// Every row has to be able to answer for itself when Enter is pressed on
    /// it, or the copy would put the wrong thing on the clipboard.
    #[test]
    fn every_row_carries_the_number_it_shows() {
        for row in rows("12 kg") {
            let result = row.result.expect("a row with a number typed has a result");
            assert!(row.label.contains(&result), "{} does not show {result}", row.label);
        }
    }
}
