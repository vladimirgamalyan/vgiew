// SVG support (ADR 0016): parse a document to a usvg tree once, then rasterize only the
// visible viewport at the current zoom. The whole module sits behind the `svg` feature, so
// a build without it links no vector renderer at all.
//
// This module deliberately knows nothing about windows, views or the framebuffer's
// conventions — it turns an SVG into pixels and reports the document's size. Deciding what
// to draw, and when, stays in main.rs.

use std::path::Path;

use rayon::prelude::*;
use resvg::{tiny_skia, usvg};

/// A parsed SVG document, ready to be rasterized at any scale.
pub struct Doc {
    tree: usvg::Tree,
    /// The document's size in SVG user units, as usvg resolves it: the declared width and
    /// height if absolute, else the viewBox, else the content's bounding box, else 100x100.
    pub w: u32,
    pub h: u32,
    /// True when the root `<svg>` gives both width and height as absolute lengths — i.e.
    /// the file states the pixel size it wants to be seen at. A percentage or a missing
    /// attribute means it does not, and ADR 0017 then fits it to the window instead of
    /// capping it at 1:1.
    pub declared: bool,
}

/// Reads and parses `path`, returning None if it is not an SVG at all. Detection is by
/// content, as it is for every other format: gzip magic marks a candidate SVGZ, and after
/// decompression the document must actually have an `<svg>` root.
pub fn open(path: &Path) -> Option<Doc> {
    let src = source(std::fs::read(path).ok()?)?;
    let tag = root_tag(&src)?;
    let declared = is_absolute_length(attr(tag, b"width")) && is_absolute_length(attr(tag, b"height"));

    let mut opt = usvg::Options {
        // Lets an <image href="sibling.png"> inside the document resolve, as it does in a
        // browser opening the same file.
        resources_dir: path.parent().map(|p| p.to_path_buf()),
        ..usvg::Options::default()
    };
    // Loading the system font database costs ~1.7 s cold and 15-18 ms warm, so it is paid
    // only by documents that actually contain text — most SVGs (icons, logos, chart
    // exports) contain none.
    if has_text(&src) {
        opt.fontdb_mut().load_system_fonts();
    }

    let tree = usvg::Tree::from_data(&src, &opt).ok()?;
    let size = tree.size().to_int_size();
    Some(Doc {
        tree,
        w: size.width(),
        h: size.height(),
        declared,
    })
}

impl Doc {
    /// Rasterizes the visible viewport: `scale` screen pixels per user unit, with the
    /// document point (cx, cy) at the centre of a `ww` x `wh` window. Returns `ww * wh`
    /// pixels as 0xAARRGGBB (straight alpha), or None if the viewport is unusable.
    ///
    /// Only the viewport is rendered, never the whole canvas — cost then tracks the visible
    /// area, and the buffer stays the window's size at any zoom. Rasterizing the full canvas
    /// at 64x on a 1000x1000 drawing would need 16 GB.
    pub fn rasterize(&self, ww: u32, wh: u32, scale: f32, cx: f32, cy: f32) -> Option<Vec<u32>> {
        if ww == 0 || wh == 0 {
            return None;
        }
        // Screen x = (doc x - cx) * scale + ww/2, and likewise for y.
        let xf = tiny_skia::Transform::from_translate(
            ww as f32 / 2.0 - cx * scale,
            wh as f32 / 2.0 - cy * scale,
        )
        .pre_scale(scale, scale);

        // resvg renders single-threaded, but a horizontal strip is just a shorter pixmap
        // with a shifted transform, so the viewport parallelizes directly. Measured at
        // 8-10x on 16 strips, which is what makes zooming a complex document interactive.
        let strips = rayon::current_num_threads().clamp(1, 16);
        let rows = (wh as usize).div_ceil(strips);
        let mut out = vec![0u32; ww as usize * wh as usize];
        let ok = out
            .par_chunks_mut(rows * ww as usize)
            .enumerate()
            .map(|(k, chunk)| {
                let y0 = (k * rows) as f32;
                let strip_h = (chunk.len() / ww as usize) as u32;
                let mut pm = tiny_skia::Pixmap::new(ww, strip_h)?;
                resvg::render(&self.tree, xf.post_translate(0.0, -y0), &mut pm.as_mut());
                // Premultiplied -> straight. Skipping the demultiply would darken every
                // antialiased edge, so it is part of the cost, not an optimisation.
                for (dst, src) in chunk.iter_mut().zip(pm.pixels()) {
                    let c = src.demultiply();
                    *dst = ((c.alpha() as u32) << 24)
                        | ((c.red() as u32) << 16)
                        | ((c.green() as u32) << 8)
                        | c.blue() as u32;
                }
                Some(())
            })
            .all(|r| r.is_some());
        ok.then_some(out)
    }

    /// Rasterizes the whole document at one user unit per pixel — the size the title
    /// reports, and what `Ctrl+C` puts on the clipboard so a paste does not depend on the
    /// current zoom (ADR 0017).
    pub fn rasterize_document(&self) -> Option<Vec<u32>> {
        self.rasterize(self.w, self.h, 1.0, self.w as f32 / 2.0, self.h as f32 / 2.0)
    }
}

/// Gunzips an SVGZ, passes plain SVG through. usvg would decompress internally, but it does
/// not expose the result, and the declared-size test needs the raw root element.
fn source(data: Vec<u8>) -> Option<Vec<u8>> {
    if !data.starts_with(&[0x1f, 0x8b]) {
        return Some(data);
    }
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&data[..]), &mut out).ok()?;
    Some(out)
}

/// Whether the document contains text, and so needs the font database. One pass, checking
/// only at `<`, because a 10 MB drawing must not pay for a naive windowed scan.
fn has_text(src: &[u8]) -> bool {
    src.iter().enumerate().any(|(i, &b)| {
        b == b'<' && (src[i..].starts_with(b"<text") || src[i..].starts_with(b"<tspan"))
    })
}

// How far into a file the root element is looked for. Generous because a leading licence
// comment is common in exported SVGs, and bounded so a huge non-SVG file is cheap to reject.
const PROLOG_LIMIT: usize = 8192;

/// The attribute span of the root `<svg>` element — everything between the tag name and the
/// closing `>`. None if the document does not start with an SVG root, which doubles as the
/// content test in `sniff`.
fn root_tag(data: &[u8]) -> Option<&[u8]> {
    let mut s = data;
    if s.starts_with(&[0xEF, 0xBB, 0xBF]) {
        s = &s[3..];
    }
    s = &s[..s.len().min(PROLOG_LIMIT)];

    // Step over whitespace, the XML declaration, comments, DOCTYPE and processing
    // instructions until the root element.
    let mut i = 0;
    loop {
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
        let rest = s.get(i..)?;
        let (open, close): (&[u8], &[u8]) = if rest.starts_with(b"<!--") {
            (b"<!--", b"-->")
        } else if rest.starts_with(b"<?") {
            (b"<?", b"?>")
        } else if rest.starts_with(b"<!") {
            (b"<!", b">")
        } else {
            break;
        };
        i += open.len() + find(&rest[open.len()..], close)? + close.len();
    }

    let rest = s.get(i..)?;
    if !rest.starts_with(b"<") {
        return None;
    }
    // The tag name, which is `svg` or ends in `:svg` when a namespace prefix is used.
    let name_end = rest[1..]
        .iter()
        .position(|&b| b.is_ascii_whitespace() || b == b'>' || b == b'/')?
        + 1;
    let name = &rest[1..name_end];
    if name != b"svg" && !name.ends_with(b":svg") {
        return None;
    }
    let tag_end = name_end + find(&rest[name_end..], b">")?;
    Some(&rest[name_end..tag_end])
}

/// The value of attribute `name` in a tag's attribute span. Matches whole names only, so
/// `stroke-width` is never mistaken for `width`.
fn attr<'a>(tag: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let mut i = 0;
    while i + name.len() < tag.len() {
        let at_name_start = i == 0 || tag[i - 1].is_ascii_whitespace();
        if at_name_start && tag[i..].starts_with(name) {
            let mut j = i + name.len();
            while j < tag.len() && tag[j].is_ascii_whitespace() {
                j += 1;
            }
            if tag.get(j) == Some(&b'=') {
                j += 1;
                while j < tag.len() && tag[j].is_ascii_whitespace() {
                    j += 1;
                }
                let quote = *tag.get(j)?;
                if quote == b'"' || quote == b'\'' {
                    j += 1;
                    let end = j + find(&tag[j..], &[quote])?;
                    return Some(&tag[j..end]);
                }
            }
        }
        i += 1;
    }
    None
}

/// Is this attribute value an absolute length? A percentage, `auto`, or a missing attribute
/// all mean the document declares no size of its own.
fn is_absolute_length(v: Option<&[u8]>) -> bool {
    let Some(v) = v else { return false };
    let t: &[u8] = {
        let start = v.iter().position(|b| !b.is_ascii_whitespace());
        match start {
            Some(s) => {
                let e = v.iter().rposition(|b| !b.is_ascii_whitespace()).unwrap_or(s);
                &v[s..=e]
            }
            None => return false,
        }
    };
    matches!(t.first(), Some(c) if c.is_ascii_digit() || *c == b'+' || *c == b'-' || *c == b'.')
        && t.last() != Some(&b'%')
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &str) -> Option<&[u8]> {
        root_tag(s.as_bytes())
    }

    #[test]
    fn detects_an_svg_root_past_any_prolog() {
        assert!(tag(r#"<svg width="1"/>"#).is_some());
        assert!(tag("\n\t <svg>").is_some());
        assert!(tag(r#"<?xml version="1.0"?><svg>"#).is_some());
        assert!(tag("<!-- a licence --><!DOCTYPE svg><svg>").is_some());
        assert!(tag("<ns0:svg>").is_some());
        assert!(tag("\u{feff}<svg>").is_some());
    }

    #[test]
    fn rejects_what_is_not_an_svg_document() {
        assert!(tag("<html><body><svg></svg>").is_none());
        assert!(tag("not xml at all").is_none());
        assert!(tag("<svgx>").is_none());
        assert!(tag("").is_none());
        assert!(tag("<!-- unterminated").is_none());
    }

    #[test]
    fn reads_whole_attribute_names_only() {
        let t = tag(r#"<svg stroke-width="9" width='400' height = "200">"#).unwrap();
        assert_eq!(attr(t, b"width"), Some(&b"400"[..]));
        assert_eq!(attr(t, b"height"), Some(&b"200"[..]));
        assert_eq!(attr(t, b"viewBox"), None);
    }

    #[test]
    fn declared_size_means_both_axes_absolute() {
        let declared = |s: &str| {
            let t = tag(s).unwrap();
            is_absolute_length(attr(t, b"width")) && is_absolute_length(attr(t, b"height"))
        };
        assert!(declared(r#"<svg width="400" height="200">"#));
        assert!(declared(r#"<svg width="210mm" height="297mm">"#));
        assert!(declared(r#"<svg width=".5in" height="2e1pt">"#));
        // No size of its own: a percentage on either axis, `auto`, or nothing at all.
        assert!(!declared(r#"<svg width="100%" height="100%" viewBox="0 0 300 150">"#));
        assert!(!declared(r#"<svg width="400" height="100%">"#));
        assert!(!declared(r#"<svg width="auto" height="auto">"#));
        assert!(!declared(r#"<svg viewBox="0 0 300 150">"#));
    }

    #[test]
    fn text_scan_finds_only_text_elements() {
        assert!(has_text(b"<svg><text>hi</text></svg>"));
        assert!(has_text(b"<svg><tspan>hi</tspan></svg>"));
        assert!(!has_text(b"<svg><rect/></svg>"));
        // A path whose id merely contains the word must not drag in the font database.
        assert!(!has_text(br#"<svg><path id="textured"/></svg>"#));
    }
}
