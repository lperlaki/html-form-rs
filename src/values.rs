//! A form's fields untyped: an ordered multi-map of `name` → `value`.

/// A form's fields as name/value pairs, in the shape a submission arrives in.
///
/// A submission is where these come from most often — [`Values::parse`] builds
/// one exactly as it came off the wire — but it is the crate's one carrier for
/// "fully-qualified field name → value", so it is also what an existing record
/// is written back out as ([`WebForm::to_values`](crate::WebForm::to_values))
/// and what a form generates for a render
/// ([`WebForm::generate_defaults`](crate::WebForm::generate_defaults)).
///
/// Order is preserved and a name may repeat, which is how checkbox groups and
/// `<select multiple>` submit their values.
///
/// ```
/// use web_form::Values;
///
/// let v = Values::parse("email=a%40b.com&tag=x&tag=y");
/// assert_eq!(v.get("email"), Some("a@b.com"));
/// assert_eq!(v.all("tag").collect::<Vec<_>>(), ["x", "y"]);
///
/// // The same, straight off the wire.
/// assert_eq!(Values::parse_bytes(b"email=a%40b.com"), Values::parse("email=a%40b.com"));
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

    /// Parse a body that has not been checked for UTF-8 yet — a request body
    /// as it arrived, before anything has decided whether it is a string.
    ///
    /// Percent-decoding produces bytes whatever the body was, so the decode is
    /// where the encoding matters, and it is lossy: an invalid sequence becomes
    /// `U+FFFD` rather than rejecting the whole submission. That is what
    /// `form_urlencoded` does, and it means a single mangled byte costs one
    /// field its value rather than costing the user their form.
    pub fn parse_bytes(encoded: &[u8]) -> Self {
        let encoded = encoded.strip_prefix(b"?").unwrap_or(encoded);
        Self {
            pairs: form_urlencoded::parse(encoded).into_owned().collect(),
        }
    }

    /// Collect from any iterator of key/value pairs, e.g. the output of a
    /// framework's own body parser.
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

    /// Append a value, keeping any value already stored under the same name.
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

    /// Whether the name appears at all — distinct from being submitted empty.
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

    /// Re-encode as `application/x-www-form-urlencoded`.
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

/// Whether a submitted value counts as "not filled in".
///
/// The browser only checks for the empty string, but a whitespace-only answer
/// to a required question is never what the caller wanted.
pub(crate) fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}
