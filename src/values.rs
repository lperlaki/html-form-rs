//! A form's fields untyped: an ordered multi-map of `name` to `value`.

use std::fmt;

use serde::de::{Deserializer, MapAccess, SeqAccess, Unexpected, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

/// A form's fields as name/value pairs, in the shape a submission arrives in.
///
/// Most often these come from a submission, and [`Values::parse`] builds one
/// exactly as it came off the wire. It is also the crate's one carrier for
/// "fully qualified field name to value". So it is what
/// [`Form::to_values`](crate::Form::to_values) writes an existing record out
/// as, and what
/// [`Form::generate_defaults`](crate::Form::generate_defaults) produces for a
/// render.
///
/// The order stays, and a name may repeat. That is how a checkbox group and a
/// `<select multiple>` submit their values.
///
/// ```
/// use html_form::Values;
///
/// let v = Values::parse("email=a%40b.com&tag=x&tag=y");
/// assert_eq!(v.get("email"), Some("a@b.com"));
/// assert_eq!(v.all("tag").collect::<Vec<_>>(), ["x", "y"]);
///
/// // The same, straight off the wire.
/// assert_eq!(Values::parse_bytes(b"email=a%40b.com"), Values::parse("email=a%40b.com"));
/// ```
///
/// # As JSON
///
/// `Values` is `Serialize` and `Deserialize`, so the same form serves a JSON
/// API and an HTML page. The submission is the only thing that differs, and it
/// stops differing here. See the [`Serialize`] and [`Deserialize`] impls for
/// what each shape means.
///
/// ```
/// use html_form::{Form, Values};
///
/// #[derive(Form)]
/// struct Signup {
///     #[field(type = "email")]
///     email: String,
///     age: Option<u32>,
///     tags: Vec<String>,
/// }
///
/// let values: Values = serde_json::from_str(
///     r#"{"email": "ada@example.com", "age": 36, "tags": ["rust", "forms"]}"#,
/// )
/// .unwrap();
///
/// let signup = Signup::from_values(&values).unwrap();
/// assert_eq!(signup.age, Some(36));
/// assert_eq!(signup.tags, ["rust", "forms"]);
///
/// // And back out, for a client that wants JSON in place of markup.
/// let json = serde_json::to_string(&signup.to_values()).unwrap();
/// assert_eq!(json, r#"{"email":"ada@example.com","age":"36","tags":["rust","forms"]}"#);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values {
    pairs: Vec<(String, String)>,
}

impl Values {
    pub const fn new() -> Self {
        Self { pairs: Vec::new() }
    }

    /// Parse an `application/x-www-form-urlencoded` body or query string.
    pub fn parse(encoded: &str) -> Self {
        Self::parse_bytes(encoded.as_bytes())
    }

    /// Parse a body that nothing has checked for UTF-8 yet: a request body as
    /// it arrived, before anything decided whether it is a string.
    ///
    /// Percent-decoding produces bytes whatever the body was, so the encoding
    /// matters at the decode. The decode is lossy. An invalid sequence becomes
    /// `U+FFFD`, and the crate does not reject the whole submission. That is
    /// what `form_urlencoded` does, and it means one broken byte costs one
    /// field its value in place of costing the user their form.
    pub fn parse_bytes(encoded: &[u8]) -> Self {
        let encoded = encoded.strip_prefix(b"?").unwrap_or(encoded);
        Self {
            pairs: form_urlencoded::parse(encoded).into_owned().collect(),
        }
    }

    /// Collect from any iterator of key and value pairs, such as the output of
    /// a framework's own body parser.
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            pairs: pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// Add a value, and keep any value already stored under the same name.
    pub fn push(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.pairs.push((name.into(), value.into()));
    }

    /// Replace every value stored under `name`.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        self.pairs.retain(|(k, _)| *k != name);
        self.pairs.push((name, value.into()));
    }

    /// The first value submitted under `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Every value submitted under `name`, in submission order.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.pairs
            .iter()
            .filter(move |(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether the name appears at all. This differs from an empty submission.
    pub fn contains(&self, name: &str) -> bool {
        self.pairs.iter().any(|(k, _)| k == name)
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pairs.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Encode again as `application/x-www-form-urlencoded`.
    pub fn to_urlencoded(&self) -> String {
        let mut ser = form_urlencoded::Serializer::new(String::new());
        for (k, v) in &self.pairs {
            ser.append_pair(k, v);
        }
        ser.finish()
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for Values {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self::from_pairs(iter)
    }
}

impl Values {
    /// The values of each name, in the order the names first appear.
    ///
    /// This is the grouping that serialization needs, and the one thing the
    /// shape of this type does not give. A name that repeats is several pairs
    /// here, and one entry there.
    // A linear lookup, not a map. A form has a handful of fields, and this
    // keeps the names in submission order without a sort.
    fn grouped(&self) -> Vec<(&str, Vec<&str>)> {
        let mut grouped: Vec<(&str, Vec<&str>)> = Vec::new();
        for (name, value) in &self.pairs {
            match grouped.iter_mut().find(|(k, _)| *k == name) {
                Some((_, values)) => values.push(value),
                None => grouped.push((name, vec![value])),
            }
        }
        grouped
    }
}

impl Serialize for Values {
    /// Serializes as an object of `name` to value. A name submitted more than
    /// once carries the list of its values:
    /// `{"email": "a@b.com", "tag": ["x", "y"]}`.
    ///
    /// The names keep the order the client submitted them in, and so do the
    /// values under each name. A repeated name appears once, where its *first*
    /// value stood, so a round trip through JSON puts the repeats together.
    /// That is the one thing an object cannot say, and no form reads anything
    /// into the distance between two checkboxes of the same group.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let grouped = self.grouped();
        let mut map = serializer.serialize_map(Some(grouped.len()))?;
        for (name, values) in grouped {
            // A single value is a string, not a list of one. A client gets back
            // what it sent, and nobody wrote `{"email": ["a@b.com"]}`.
            match values.as_slice() {
                [only] => map.serialize_entry(name, only)?,
                many => map.serialize_entry(name, many)?,
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Values {
    /// Deserializes from either shape a submission takes. One is an object of
    /// `name` to value. The other is a list of `[name, value]` pairs, which
    /// keeps every repeat exactly where it was.
    ///
    /// A value may be a string, a number or a boolean. A form field is a string
    /// whatever type a JSON client used, so `{"age": 36}` and `{"age": "36"}`
    /// are the same submission. A list is a name submitted more than once, as a
    /// checkbox group submits it.
    ///
    /// `null` is *no* value, not an empty one, so it leaves the name out. That
    /// is what an absent field means, defaults included. To clear a field, a
    /// client sends `""`, exactly as the browser does.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValuesVisitor)
    }
}

struct ValuesVisitor;

impl<'de> Visitor<'de> for ValuesVisitor {
    type Value = Values;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object of form fields, or a list of name/value pairs")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Values, A::Error> {
        let mut values = Values::new();
        while let Some((name, submitted)) = map.next_entry::<String, Submitted>()? {
            submitted.push_into(&mut values, name);
        }
        Ok(values)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Values, A::Error> {
        let mut values = Values::new();
        while let Some((name, value)) = seq.next_element::<(String, Scalar)>()? {
            if let Some(value) = value.0 {
                values.push(name, value);
            }
        }
        Ok(values)
    }
}

/// What one name carries in a deserialized object: one value, or the several
/// that a repeated name submits.
enum Submitted {
    One(Scalar),
    Many(Vec<Scalar>),
}

impl Submitted {
    fn push_into(self, values: &mut Values, name: String) {
        match self {
            Submitted::One(Scalar(Some(value))) => values.push(name, value),
            Submitted::One(Scalar(None)) => {}
            Submitted::Many(many) => {
                for Scalar(value) in many {
                    if let Some(value) = value {
                        // Cloned per value, because each repeat is its own pair.
                        values.push(name.clone(), value);
                    }
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for Submitted {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SubmittedVisitor)
    }
}

struct SubmittedVisitor;

/// Everything a lone value may be, passed straight to [`ScalarVisitor`]. Only
/// the list case belongs to this visitor. A name with one value and a name with
/// several differ nowhere else.
macro_rules! visit_scalar {
    ($($method:ident($($arg:ident: $ty:ty)?)),* $(,)?) => {$(
        fn $method<E: serde::de::Error>(self $(, $arg: $ty)?) -> Result<Submitted, E> {
            ScalarVisitor.$method($($arg)?).map(Submitted::One)
        }
    )*};
}

impl<'de> Visitor<'de> for SubmittedVisitor {
    type Value = Submitted;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a form value: a string, a number, a boolean, null, or a list of them")
    }

    visit_scalar! {
        visit_str(value: &str),
        visit_string(value: String),
        visit_i64(value: i64),
        visit_i128(value: i128),
        visit_u64(value: u64),
        visit_u128(value: u128),
        visit_f64(value: f64),
        visit_bool(value: bool),
        visit_unit(),
        visit_none(),
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Submitted, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Submitted, A::Error> {
        ScalarVisitor.visit_map(map).map(Submitted::One)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Submitted, A::Error> {
        let mut many = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(value) = seq.next_element()? {
            many.push(value);
        }
        Ok(Submitted::Many(many))
    }
}

/// One submitted value, in whatever JSON type the client used. `None` is
/// `null`: a name with nothing under it, which is a name the client did not
/// submit.
struct Scalar(Option<String>);

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ScalarVisitor)
    }
}

struct ScalarVisitor;

impl<'de> Visitor<'de> for ScalarVisitor {
    type Value = Scalar;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a form value: a string, a number, a boolean or null")
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Scalar, E> {
        Ok(Scalar(Some(value.to_owned())))
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Scalar, E> {
        Ok(Scalar(Some(value)))
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Scalar, E> {
        Ok(Scalar(Some(value.to_string())))
    }

    fn visit_i128<E: serde::de::Error>(self, value: i128) -> Result<Scalar, E> {
        Ok(Scalar(Some(value.to_string())))
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Scalar, E> {
        Ok(Scalar(Some(value.to_string())))
    }

    fn visit_u128<E: serde::de::Error>(self, value: u128) -> Result<Scalar, E> {
        Ok(Scalar(Some(value.to_string())))
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Scalar, E> {
        Ok(Scalar(Some(value.to_string())))
    }

    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Scalar, E> {
        // The two words the `FormValue` of a checkbox reads back.
        Ok(Scalar(Some(
            if value { "true" } else { "false" }.to_owned(),
        )))
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Scalar, E> {
        Ok(Scalar(None))
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Scalar, E> {
        Ok(Scalar(None))
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Scalar, D::Error> {
        deserializer.deserialize_any(self)
    }

    /// A nested object is where the crate has to state the flat namespace of a
    /// form. `billing_street` is one field, not a `street` inside a `billing`.
    fn visit_map<A: MapAccess<'de>>(self, _map: A) -> Result<Scalar, A::Error> {
        Err(serde::de::Error::invalid_type(
            Unexpected::Map,
            &"a form value: a form submits a flat list of names, so a nested \
              object has no name to be submitted under — flatten it with \
              `#[field(flatten, prefix = \"…\")]` and send `billing_street`",
        ))
    }
}

/// Whether a submitted value counts as "not filled in".
///
/// The browser checks only for the empty string. But a whitespace-only answer
/// to a required question is never what the caller wanted.
pub(crate) fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}
