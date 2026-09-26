//! Keys for caches of text measurements: a string and the parameters it was
//! measured with. A stored key owns its text; a lookup borrows the caller's,
//! so probing a cache never copies the string it asks about.

use std::{
    borrow::Borrow,
    hash::{Hash, Hasher},
    rc::Rc,
};

/// What a text cache key hashes and compares: its text and its parameters.
/// Stored keys and borrowed probes both implement it, so they hash and
/// compare alike.
pub(crate) trait TextKey<P> {
    fn text(&self) -> &str;
    fn params(&self) -> &P;
}

/// A key a cache stores: it owns its text.
#[derive(Clone)]
pub(crate) struct TextCacheKey<P> {
    text: Rc<str>,
    params: P,
}

impl<P> TextCacheKey<P> {
    /// A key for `text`, copying it once for the cache to keep.
    pub(crate) fn new(text: &str, params: P) -> Self {
        Self {
            text: Rc::from(text),
            params,
        }
    }
}

/// A lookup for `text` with `params`, borrowing the caller's text.
pub(crate) struct TextProbe<'a, P> {
    text: &'a str,
    params: P,
}

impl<'a, P> TextProbe<'a, P> {
    pub(crate) fn new(text: &'a str, params: P) -> Self {
        Self { text, params }
    }

    /// The probe as the cache's borrowed key type.
    pub(crate) fn key(&self) -> &(dyn TextKey<P> + 'a)
    where
        P: 'a,
    {
        self
    }

    /// The key a cache stores for this probe's text and parameters.
    pub(crate) fn to_owned_key(&self) -> TextCacheKey<P>
    where
        P: Clone,
    {
        TextCacheKey::new(self.text, self.params.clone())
    }
}

impl<P> TextKey<P> for TextCacheKey<P> {
    fn text(&self) -> &str {
        &self.text
    }

    fn params(&self) -> &P {
        &self.params
    }
}

impl<P> TextKey<P> for TextProbe<'_, P> {
    fn text(&self) -> &str {
        self.text
    }

    fn params(&self) -> &P {
        &self.params
    }
}

impl<P: Hash> Hash for dyn TextKey<P> + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text().hash(state);
        self.params().hash(state);
    }
}

impl<P: PartialEq> PartialEq for dyn TextKey<P> + '_ {
    fn eq(&self, other: &Self) -> bool {
        let (text, other_text) = (self.text(), other.text());
        (std::ptr::eq(text, other_text) || text == other_text) && self.params() == other.params()
    }
}

impl<P: Eq> Eq for dyn TextKey<P> + '_ {}

impl<P: Hash> Hash for TextCacheKey<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self as &dyn TextKey<P>).hash(state);
    }
}

impl<P: PartialEq> PartialEq for TextCacheKey<P> {
    fn eq(&self, other: &Self) -> bool {
        (self as &dyn TextKey<P>) == (other as &dyn TextKey<P>)
    }
}

impl<P: Eq> Eq for TextCacheKey<P> {}

impl<'a, P: 'a> Borrow<dyn TextKey<P> + 'a> for TextCacheKey<P> {
    fn borrow(&self) -> &(dyn TextKey<P> + 'a) {
        self
    }
}

#[cfg(test)]
#[path = "tests/text_cache_key_tests.rs"]
mod tests;
