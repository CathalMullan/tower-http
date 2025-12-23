//! URI reference resolution.
//! <https://www.rfc-editor.org/rfc/rfc3986#section-5>

use std::{borrow::Cow, convert::TryFrom};

use http::Uri;

/// Try to resolve a URI reference against a base URI.
/// <https://www.rfc-editor.org/rfc/rfc3986#section-5.2.2>
pub(super) fn resolve_uri(reference: &str, base: &Uri) -> Option<Uri> {
    let reference = reference.trim();

    // Encode any invalid chars.
    // TODO: Encode by component, rather than the full reference?
    let reference = encode_uri(reference);
    let reference = reference.as_ref();

    // Strip fragments.
    let reference = reference
        .split_once('#')
        .map_or(reference, |(before, _)| before);

    // Try and parse up front.
    if let Ok(uri) = Uri::try_from(reference) {
        // Use if it has a scheme.
        if uri.scheme().is_some() {
            return Some(uri);
        }

        // Reject unrecognized schemes.
        if let Some((before_colon, _)) = reference.split_once(':') {
            if !before_colon.contains('/') {
                return None;
            }
        }
    }

    let scheme = base.scheme_str()?;
    let authority = base.authority()?;

    // Reference has its own authority.
    if reference.starts_with("//") {
        return Uri::try_from(format!("{scheme}:{reference}")).ok();
    }

    let (path, query) = match reference.find('?') {
        Some(index) => (&reference[..index], &reference[index..]),
        None => (reference, ""),
    };

    // Resolve path:
    // - empty uses base
    // - absolute uses reference
    // - relative merges both
    let path = if path.is_empty() {
        if query.is_empty() {
            return Some(base.clone());
        }

        base.path().to_string()
    } else if path.starts_with('/') {
        remove_dot_segments(path)
    } else {
        let merged = match base.path().rfind('/') {
            Some(index) => {
                let base = &base.path()[..=index];
                format!("{base}{path}")
            }
            None => format!("/{path}"),
        };

        remove_dot_segments(&merged)
    };

    Uri::builder()
        .scheme(scheme)
        .authority(authority.clone())
        .path_and_query(path + query)
        .build()
        .ok()
}

/// Only percent encode characters that `http::Uri` rejects.
fn encode_uri(reference: &str) -> Cow<'_, str> {
    if !reference
        .bytes()
        .any(|byte| matches!(byte, b' ' | b'<' | b'>' | b'`'))
    {
        return Cow::Borrowed(reference);
    }

    let mut result = String::with_capacity(reference.len());
    for char in reference.chars() {
        match char {
            ' ' => result.push_str("%20"),
            '<' => result.push_str("%3C"),
            '>' => result.push_str("%3E"),
            '`' => result.push_str("%60"),
            _ => result.push(char),
        }
    }

    Cow::Owned(result)
}

/// Remove `.` and `..` segments from a path.
/// <https://www.rfc-editor.org/rfc/rfc3986#section-5.2.4>
fn remove_dot_segments(path: &str) -> String {
    let absolute = path.starts_with('/');

    let mut segments: Vec<&str> = Vec::new();
    let mut trailing_slash = false;

    for segment in path.split('/') {
        match segment {
            "." => trailing_slash = true,
            ".." => {
                if segments.last() != Some(&"") {
                    segments.pop();
                }

                trailing_slash = true;
            }
            segment => {
                segments.push(segment);
                trailing_slash = false;
            }
        }
    }

    let result = segments.join("/");
    if trailing_slash {
        if result.is_empty() {
            return if absolute { "/".into() } else { String::new() };
        }

        if !result.ends_with('/') {
            return format!("{result}/");
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_resolve_uri {
        ($base:expr, $reference:expr, $expected:expr $(,)?) => {
            assert_eq!(
                resolve_uri($reference, &Uri::from_static($base)),
                Into::<Option<&'static str>>::into($expected).map(Uri::from_static),
            )
        };
    }

    /// All normal "Reference Resolution Examples" from RFC 3986.
    /// <https://www.rfc-editor.org/rfc/rfc3986#section-5.4.1>
    #[test]
    fn resolve_rfc_normal() {
        let base = "http://a/b/c/d;p?q";

        // "g:h" = "g:h"
        // NOTE: `http::Uri` only supports HTTP protocols.
        assert_resolve_uri!(base, "g:h", None);

        // "g" = "http://a/b/c/g"
        assert_resolve_uri!(base, "g", "http://a/b/c/g");

        // "./g" = "http://a/b/c/g"
        assert_resolve_uri!(base, "./g", "http://a/b/c/g");

        // "g/" = "http://a/b/c/g/"
        assert_resolve_uri!(base, "g/", "http://a/b/c/g/");

        // "/g" = "http://a/g"
        assert_resolve_uri!(base, "/g", "http://a/g");

        // "//g" = "http://g"
        // NOTE: `http::Uri` adds a trailing slash.
        assert_resolve_uri!(base, "//g", "http://g/");

        // "?y" = "http://a/b/c/d;p?y"
        assert_resolve_uri!(base, "?y", "http://a/b/c/d;p?y");

        // "g?y" = "http://a/b/c/g?y"
        assert_resolve_uri!(base, "g?y", "http://a/b/c/g?y");

        // "#s" = "http://a/b/c/d;p?q#s"
        // NOTE: `http::Uri` doesn't support fragments.
        assert_resolve_uri!(base, "#s", "http://a/b/c/d;p?q");

        // "g#s" = "http://a/b/c/g#s"
        // NOTE: `http::Uri` doesn't support fragments.
        assert_resolve_uri!(base, "g#s", "http://a/b/c/g");

        // "g?y#s" = "http://a/b/c/g?y#s"
        // NOTE: `http::Uri` doesn't support fragments.
        assert_resolve_uri!(base, "g?y#s", "http://a/b/c/g?y");

        // ";x" = "http://a/b/c/;x"
        assert_resolve_uri!(base, ";x", "http://a/b/c/;x");

        // "g;x" = "http://a/b/c/g;x"
        assert_resolve_uri!(base, "g;x", "http://a/b/c/g;x");

        // "g;x?y#s" = "http://a/b/c/g;x?y#s"
        // NOTE: `http::Uri` doesn't support fragments.
        assert_resolve_uri!(base, "g;x?y#s", "http://a/b/c/g;x?y");

        // "" = "http://a/b/c/d;p?q"
        assert_resolve_uri!(base, "", "http://a/b/c/d;p?q");

        // "." = "http://a/b/c/"
        assert_resolve_uri!(base, ".", "http://a/b/c/");

        // "./" = "http://a/b/c/"
        assert_resolve_uri!(base, "./", "http://a/b/c/");

        // ".." = "http://a/b/"
        assert_resolve_uri!(base, "..", "http://a/b/");

        // "../" = "http://a/b/"
        assert_resolve_uri!(base, "../", "http://a/b/");

        // "../g" = "http://a/b/g"
        assert_resolve_uri!(base, "../g", "http://a/b/g");

        // "../.." = "http://a/"
        assert_resolve_uri!(base, "../..", "http://a/");

        // "../../" = "http://a/"
        assert_resolve_uri!(base, "../../", "http://a/");

        // "../../g" = "http://a/g"
        assert_resolve_uri!(base, "../../g", "http://a/g");
    }

    /// All abnormal "Reference Resolution Examples" from RFC 3986.
    /// <https://www.rfc-editor.org/rfc/rfc3986#section-5.4.2>
    #[test]
    fn resolve_rfc_abnormal() {
        let base = "http://a/b/c/d;p?q";

        // "../../../g" = "http://a/g"
        assert_resolve_uri!(base, "../../../g", "http://a/g");

        // "../../../../g" = "http://a/g"
        assert_resolve_uri!(base, "../../../../g", "http://a/g");

        // "/./g" = "http://a/g"
        assert_resolve_uri!(base, "/./g", "http://a/g");

        // "/../g" = "http://a/g"
        assert_resolve_uri!(base, "/../g", "http://a/g");

        // "g." = "http://a/b/c/g."
        assert_resolve_uri!(base, "g.", "http://a/b/c/g.");

        // ".g" = "http://a/b/c/.g"
        assert_resolve_uri!(base, ".g", "http://a/b/c/.g");

        // "g.." = "http://a/b/c/g.."
        assert_resolve_uri!(base, "g..", "http://a/b/c/g..");

        // "..g" = "http://a/b/c/..g"
        assert_resolve_uri!(base, "..g", "http://a/b/c/..g");

        // "./../g" = "http://a/b/g"
        assert_resolve_uri!(base, "./../g", "http://a/b/g");

        // "./g/." = "http://a/b/c/g/"
        assert_resolve_uri!(base, "./g/.", "http://a/b/c/g/");

        // "g/./h" = "http://a/b/c/g/h"
        assert_resolve_uri!(base, "g/./h", "http://a/b/c/g/h");

        // "g/../h" = "http://a/b/c/h"
        assert_resolve_uri!(base, "g/../h", "http://a/b/c/h");

        // "g;x=1/./y" = "http://a/b/c/g;x=1/y"
        assert_resolve_uri!(base, "g;x=1/./y", "http://a/b/c/g;x=1/y");

        // "g;x=1/../y" = "http://a/b/c/y"
        assert_resolve_uri!(base, "g;x=1/../y", "http://a/b/c/y");

        // "g?y/./x" = "http://a/b/c/g?y/./x"
        assert_resolve_uri!(base, "g?y/./x", "http://a/b/c/g?y/./x");

        // "g?y/../x" = "http://a/b/c/g?y/../x"
        assert_resolve_uri!(base, "g?y/../x", "http://a/b/c/g?y/../x");

        // "g#s/./x" = "http://a/b/c/g#s/./x"
        // NOTE: `http::Uri` doesn't support fragments.
        assert_resolve_uri!(base, "g#s/./x", "http://a/b/c/g");

        // "g#s/../x" = "http://a/b/c/g#s/../x"
        // NOTE: `http::Uri` doesn't support fragments.
        assert_resolve_uri!(base, "g#s/../x", "http://a/b/c/g");

        // "http:g" = "http:g"
        // NOTE: `http::Uri` only supports HTTP protocols.
        assert_resolve_uri!(base, "http:g", None);
    }

    /// Hand-picked edge cases from the Web Platform Tests.
    /// <https://github.com/web-platform-tests/wpt/blob/master/url/resources/urltestdata.json>
    #[test]
    fn resolve_wpt() {
        // "/a/ /c" = "http://example.org/a/%20/c"
        assert_resolve_uri!(
            "http://example.org/foo/bar",
            "/a/ /c",
            "http://example.org/a/%20/c"
        );

        // "/a%2fc" = "http://example.org/a%2fc"
        assert_resolve_uri!(
            "http://example.org/foo/bar",
            "/a%2fc",
            "http://example.org/a%2fc"
        );

        // "/a/%2f/c" = "http://example.org/a/%2f/c"
        assert_resolve_uri!(
            "http://example.org/foo/bar",
            "/a/%2f/c",
            "http://example.org/a/%2f/c"
        );

        // " foo.com  " = "http://example.org/foo/foo.com"
        assert_resolve_uri!(
            "http://example.org/foo/bar",
            " foo.com  ",
            "http://example.org/foo/foo.com"
        );

        // "/path" = "http://user@example.org/path"
        assert_resolve_uri!(
            "http://user@example.org/smth",
            "/path",
            "http://user@example.org/path"
        );

        // "/path" = "http://user:pass@example.org:21/path"
        assert_resolve_uri!(
            "http://user:pass@example.org:21/smth",
            "/path",
            "http://user:pass@example.org:21/path"
        );

        // "http://`{}:`{}@h/`{}?`{}" = "http://%60%7B%7D:%60%7B%7D@h/%60%7B%7D?`{}"
        assert_resolve_uri!(
            "http://doesnotmatter/",
            "http://`{}:`{}@h/`{}?`{}",
            "http://%60%7B%7D:%60%7B%7D@h/%60%7B%7D?`{}"
        );
    }

    /// All "Remove Dot Segments" examples from RFC 3986.
    /// <https://www.rfc-editor.org/rfc/rfc3986#section-5.2.4>
    #[test]
    fn remove_dot_segments_rfc() {
        assert_eq!(remove_dot_segments("/a/b/c/./../../g"), "/a/g");
        assert_eq!(remove_dot_segments("mid/content=5/../6"), "mid/6");
    }
}
