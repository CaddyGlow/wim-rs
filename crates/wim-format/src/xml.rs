//! WIM image properties and the upstream XML dialect, without native libraries.
//! Unknown elements, attributes and mixed text are retained; comments and PIs
//! are discarded, matching xmlproc.c. Only five named entities are supported.
use crate::ParseError;
use crate::allocation::*;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Child {
    Element(Element),
    Text(Vec<u8>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Element {
    name: Vec<u8>,
    attrs: Vec<(Vec<u8>, Vec<u8>)>,
    children: Vec<Child>,
}
impl Element {
    fn empty(name: &[u8]) -> Result<Self, ParseError> {
        Ok(Self {
            name: copy_owned_bytes(name)?,
            attrs: Vec::new(),
            children: Vec::new(),
        })
    }
    fn try_clone(&self) -> Result<Self, ParseError> {
        let mut result = Self::empty(b"")?;
        result.name = copy_owned_bytes(&self.name)?;
        result
            .attrs
            .try_reserve(self.attrs.len())
            .map_err(|_| ParseError::Nomem)?;
        for (name, value) in &self.attrs {
            result
                .attrs
                .try_push((copy_owned_bytes(name)?, copy_owned_bytes(value)?))
                .map_err(|_| ParseError::Nomem)?;
        }
        result
            .children
            .try_reserve(self.children.len())
            .map_err(|_| ParseError::Nomem)?;
        for child in &self.children {
            let child = match child {
                Child::Element(element) => Child::Element(element.try_clone()?),
                Child::Text(text) => Child::Text(copy_owned_bytes(text)?),
            };
            result
                .children
                .try_push(child)
                .map_err(|_| ParseError::Nomem)?;
        }
        Ok(result)
    }
    fn text(&self) -> Option<&str> {
        self.children.iter().find_map(|c| {
            if let Child::Text(t) = c {
                core::str::from_utf8(t).ok()
            } else {
                None
            }
        })
    }
    fn append(&mut self, t: Vec<u8>) -> Result<(), ParseError> {
        if t.is_empty() {
            return Ok(());
        }
        if let Some(Child::Text(last)) = self.children.last_mut() {
            last.try_reserve(t.len()).map_err(|_| ParseError::Nomem)?;
            last.try_extend_from_slice(&t)
                .map_err(|_| ParseError::Nomem)?;
        } else {
            self.children
                .try_push(Child::Text(copy_owned_bytes(&t)?))
                .map_err(|_| ParseError::Nomem)?;
        }
        Ok(())
    }
    fn find(&self, path: &[(&[u8], u32)]) -> Option<&Self> {
        let Some((&(name, n), rest)) = path.split_first() else {
            return Some(self);
        };
        self.children
            .iter()
            .filter_map(|c| {
                if let Child::Element(e) = c {
                    Some(e)
                } else {
                    None
                }
            })
            .filter(|e| e.name == name)
            .nth(n as usize - 1)?
            .find(rest)
    }
    fn ensure(&mut self, path: &[(&[u8], u32)]) -> Result<&mut Self, ParseError> {
        let Some((&(name, n), rest)) = path.split_first() else {
            return Ok(self);
        };
        let mut count = 0;
        let mut found = None;
        for (i, c) in self.children.iter().enumerate() {
            if matches!(c,Child::Element(e) if e.name == name) {
                count += 1;
                if count == n {
                    found = Some(i);
                    break;
                }
            }
        }
        let i = if let Some(i) = found {
            i
        } else {
            if n != count + 1 {
                return Err(ParseError::InvalidParam);
            }
            self.children
                .try_push(Child::Element(Self::empty(name)?))
                .map_err(|_| ParseError::Nomem)?;
            self.children.len() - 1
        };
        match &mut self.children[i] {
            Child::Element(e) => e.ensure(rest),
            Child::Text(_) => Err(ParseError::Xml),
        }
    }
    fn remove(&mut self, path: &[(&[u8], u32)]) {
        let Some((&(name, n), rest)) = path.split_first() else {
            return;
        };
        let mut count = 0;
        let i = self.children.iter().position(|c| {
            if matches!(c,Child::Element(e) if e.name==name) {
                count += 1;
                count == n
            } else {
                false
            }
        });
        if let Some(i) = i {
            if rest.is_empty() {
                self.children.remove(i);
            } else if let Child::Element(e) = &mut self.children[i] {
                e.remove(rest)
            }
        }
    }
    fn write(&self, out: &mut Vec<u8>) {
        out.push(b'<');
        out.extend_from_slice(&self.name);
        for (k, v) in &self.attrs {
            out.push(b' ');
            out.extend_from_slice(k);
            out.extend_from_slice(b"=\"");
            escape(v, out);
            out.push(b'"');
        }
        out.push(b'>');
        for child in &self.children {
            match child {
                Child::Element(element) => element.write(out),
                Child::Text(text) => escape(text, out),
            }
        }
        out.extend_from_slice(b"</");
        out.extend_from_slice(&self.name);
        out.push(b'>');
    }
    fn text_bytes(&self) -> Option<&[u8]> {
        self.children.iter().find_map(|child| match child {
            Child::Text(text) => Some(text.as_slice()),
            Child::Element(_) => None,
        })
    }
}
/// Parsed WIM XML with one-based, INDEX-ordered image access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlInfo {
    root: Box<Element>,
    images: Vec<usize>,
}
impl XmlInfo {
    /// Clone strings and indices with fallible collection growth.
    /// The document root follows the global allocator’s allocation failure policy.
    pub fn try_clone(&self) -> Result<Self, ParseError> {
        let mut root = Box::new(Element::empty(b"")?);
        *root = self.root.try_clone()?;
        let mut images = Vec::new();
        images
            .try_reserve(self.images.len())
            .map_err(|_| ParseError::Nomem)?;
        images
            .try_extend_from_slice(&self.images)
            .map_err(|_| ParseError::Nomem)?;
        Ok(Self { root, images })
    }
    /// Whether a root TOTALBYTES element exists, independently of its value.
    pub fn has_total_bytes(&self) -> bool {
        self.root.find(&[(b"TOTALBYTES", 1)]).is_some()
    }
    /// Read top-level TOTALBYTES, returning zero when absent or nonnumeric.
    pub fn total_bytes(&self) -> u64 {
        self.root
            .find(&[(b"TOTALBYTES", 1)])
            .and_then(Element::text)
            .map(number)
            .unwrap_or(0)
    }
    /// Replace the first top-level TOTALBYTES at the end, or remove it for None.
    pub fn set_total_bytes(&mut self, bytes: Option<u64>) -> Result<(), ParseError> {
        if let Some(index) = self
            .root
            .children
            .iter()
            .position(|child| matches!(child,Child::Element(e) if e.name==b"TOTALBYTES"))
        {
            self.root.children.remove(index);
            for image in &mut self.images {
                if *image > index {
                    *image -= 1;
                }
            }
        }
        if let Some(bytes) = bytes {
            self.root
                .children
                .try_reserve(1)
                .map_err(|_| ParseError::Nomem)?;
            let mut element = Element::empty(b"TOTALBYTES")?;
            element
                .children
                .try_push(Child::Text(copy_owned_bytes(bytes.to_string().as_bytes())?))
                .map_err(|_| ParseError::Nomem)?;
            self.root
                .children
                .try_push(Child::Element(element))
                .map_err(|_| ParseError::Nomem)?;
        }
        Ok(())
    }
    /// Clone selected one-based images in order, retaining unknown root data.
    /// Image INDEX attributes are renumbered; repeated indices are permitted.
    pub fn select_images(&self, indices: &[u32]) -> Result<Self, ParseError> {
        for &index in indices {
            if index == 0 || index as usize > self.images.len() {
                return Err(ParseError::InvalidImage);
            }
        }
        let mut result = self.try_clone()?;
        result
            .root
            .children
            .retain(|child| !matches!(child,Child::Element(e) if e.name==b"IMAGE"));
        result.images.clear();
        result.append_images(self, indices)?;
        Ok(result)
    }
    /// Append selected source images, renumbering INDEX and preserving properties.
    /// Name collision policy is supplied by the higher-level image operation.
    pub fn append_images(&mut self, source: &Self, indices: &[u32]) -> Result<(), ParseError> {
        if self
            .images
            .len()
            .checked_add(indices.len())
            .is_none_or(|n| n > 65535)
        {
            return Err(ParseError::ImageCount);
        }
        for &index in indices {
            if index == 0 || index as usize > source.images.len() {
                return Err(ParseError::InvalidImage);
            }
        }
        self.images
            .try_reserve(indices.len())
            .map_err(|_| ParseError::Nomem)?;
        self.root
            .children
            .try_reserve(indices.len())
            .map_err(|_| ParseError::Nomem)?;
        for &index in indices {
            let Child::Element(original) = &source.root.children[source.images[index as usize - 1]]
            else {
                return Err(ParseError::Xml);
            };
            let mut element = original.try_clone()?;
            let index = (self.images.len() + 1).to_string().into_bytes();
            if let Some((_, value)) = element
                .attrs
                .iter_mut()
                .find(|(key, _)| key.as_slice() == b"INDEX")
            {
                *value = copy_owned_bytes(&index)?;
            } else {
                element
                    .attrs
                    .try_push((copy_owned_bytes(b"INDEX")?, copy_owned_bytes(&index)?))
                    .map_err(|_| ParseError::Nomem)?;
            }
            self.images
                .try_push(self.root.children.len())
                .map_err(|_| ParseError::Nomem)?;
            self.root
                .children
                .try_push(Child::Element(element))
                .map_err(|_| ParseError::Nomem)?;
        }
        Ok(())
    }
    /// Parse the upstream XML dialect and validate contiguous image indices.
    pub fn parse(xml: &str) -> Result<Self, ParseError> {
        Self::parse_bytes(xml.as_bytes())
    }
    /// Parse original platform XML bytes, preserving WTF-8 text and names.
    pub fn parse_bytes(xml: &[u8]) -> Result<Self, ParseError> {
        let mut root = Box::new(Element::empty(b"")?);
        let xml = xml.split(|&b| b == 0).next().unwrap_or(b"");
        let mut p = Parser {
            remaining: xml.strip_prefix(b"\xef\xbb\xbf").unwrap_or(xml),
        };
        p.misc()?;
        *root = p.element(0)?;
        p.misc()?;
        if !p.remaining.is_empty() || root.name != b"WIM" {
            return Err(ParseError::Xml);
        }
        if root.find(&[(b"ESD", 1), (b"ENCRYPTED", 1)]).is_some() {
            return Err(ParseError::WimIsEncrypted);
        }
        let mut entries = Vec::new();
        for (i, c) in root.children.iter().enumerate() {
            if let Child::Element(e) = c
                && e.name == b"IMAGE"
            {
                let index = e
                    .attrs
                    .iter()
                    .find(|(k, _)| k.as_slice() == b"INDEX")
                    .and_then(|(_, v)| core::str::from_utf8(v).ok())
                    .map(number)
                    .unwrap_or(0);
                if index == 0 || index > 65535 {
                    return Err(ParseError::Xml);
                }
                entries
                    .try_push((index as usize, i))
                    .map_err(|_| ParseError::Nomem)?;
            }
        }
        if entries.len() > 65535 {
            return Err(ParseError::Xml);
        }
        let mut images = Vec::new();
        images
            .try_reserve(entries.len())
            .map_err(|_| ParseError::Nomem)?;
        for _ in 0..entries.len() {
            images.try_push(usize::MAX).map_err(|_| ParseError::Nomem)?;
        }
        for &(index, child) in &entries {
            let slot = images.get_mut(index - 1).ok_or(ParseError::Xml)?;
            if *slot != usize::MAX {
                return Err(ParseError::Xml);
            }
            *slot = child;
        }
        Ok(Self { root, images })
    }
    /// Decode UTF-16LE WIM XML, with or without a BOM.
    pub fn parse_utf16le(bytes: &[u8]) -> Result<Self, ParseError> {
        if !bytes.len().is_multiple_of(2) {
            return Err(ParseError::Xml);
        }
        let text = crate::platform_text::utf16le_to_wtf8(bytes)?;
        Self::parse_bytes(&text)
    }
    /// Number of images represented in the XML document.
    pub fn image_count(&self) -> usize {
        self.images.len()
    }
    fn image(&self, image: i32) -> Option<&Element> {
        let i = *self
            .images
            .get(usize::try_from(image).ok()?.checked_sub(1)?)?;
        if let Child::Element(e) = &self.root.children[i] {
            Some(e)
        } else {
            None
        }
    }
    /// Get the first UTF-8 text child at a slash path with one-based indices.
    /// Non-UTF-8 platform text is available through `get_property_bytes`.
    pub fn get_property(&self, image: i32, path: &str) -> Option<&str> {
        core::str::from_utf8(self.get_property_bytes(image, path.as_bytes())?).ok()
    }
    /// Get lossless byte text at a platform property path.
    pub fn get_property_bytes(&self, image: i32, path: &[u8]) -> Option<&[u8]> {
        if path.is_empty() {
            return None;
        }
        self.image(image)?
            .find(&parse_path(path).ok()?)?
            .text_bytes()
    }
    /// Test element presence independently of whether it contains text.
    pub fn has_element(&self, image: i32, path: &[u8]) -> bool {
        self.image(image)
            .and_then(|image| image.find(&parse_path(path).ok()?))
            .is_some()
    }
    /// Borrow direct child element names and their first text child in XML order.
    pub fn child_elements<'a>(
        &'a self,
        image: i32,
        path: &[u8],
    ) -> impl Iterator<Item = (&'a [u8], Option<&'a [u8]>)> {
        let element = self
            .image(image)
            .and_then(|image| image.find(&parse_path(path).ok()?));
        element.into_iter().flat_map(|element| {
            element.children.iter().filter_map(|child| match child {
                Child::Element(child) => Some((child.name.as_slice(), child.text_bytes())),
                Child::Text(_) => None,
            })
        })
    }
    /// Image name as UTF-8; invalid platform text returns None.
    pub fn name(&self, image: i32) -> Option<&str> {
        core::str::from_utf8(self.name_bytes(image)?).ok()
    }
    /// Lossless image name; a valid unnamed image returns empty bytes.
    pub fn name_bytes(&self, image: i32) -> Option<&[u8]> {
        self.image(image)?;
        Some(self.get_property_bytes(image, b"NAME").unwrap_or(b""))
    }
    /// Optional image description as UTF-8.
    pub fn description(&self, image: i32) -> Option<&str> {
        self.get_property(image, "DESCRIPTION")
    }
    /// Lossless optional image description.
    pub fn description_bytes(&self, image: i32) -> Option<&[u8]> {
        self.get_property_bytes(image, b"DESCRIPTION")
    }
    /// Whether a nonempty exact UTF-8 image name is assigned.
    pub fn name_in_use(&self, name: &str) -> bool {
        self.name_in_use_bytes(name.as_bytes())
    }
    /// Whether a nonempty exact byte image name is assigned.
    pub fn name_in_use_bytes(&self, name: &[u8]) -> bool {
        !name.is_empty()
            && (1..=self.image_count()).any(|i| self.name_bytes(i as i32) == Some(name))
    }
    /// Resolve a UTF-8 image selector using upstream decimal/name precedence.
    pub fn resolve_image(&self, selector: Option<&str>) -> i32 {
        self.resolve_image_bytes(selector.map(str::as_bytes))
    }
    /// Resolve an exact platform name, decimal index, `all`, or `*`.
    /// Leading ASCII whitespace and plus signs are accepted for numbers.
    pub fn resolve_image_bytes(&self, selector: Option<&[u8]>) -> i32 {
        let Some(selector) = selector.filter(|value| !value.is_empty()) else {
            return 0;
        };
        if selector.eq_ignore_ascii_case(b"all") || selector == b"*" {
            return -1;
        }
        let number = selector.trim_ascii_start();
        let digits = number.strip_prefix(b"+").unwrap_or(number);
        if !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) {
            match core::str::from_utf8(digits)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
            {
                Some(0) => {}
                Some(index) if index <= self.image_count() as u64 => return index as i32,
                _ => return 0,
            }
        }
        (1..=self.image_count())
            .find(|&i| self.name_bytes(i as i32) == Some(selector))
            .map_or(0, |i| i as i32)
    }
    /// Set or delete an XML property using UTF-8 text.
    pub fn set_property(
        &mut self,
        image: i32,
        path: &str,
        value: Option<&str>,
    ) -> Result<(), ParseError> {
        self.set_property_bytes(image, path.as_bytes(), value.map(str::as_bytes))
    }
    /// Set or delete a lossless byte XML property with upstream validation order.
    /// Byte values need not be UTF-8; conversion to UTF-16 fails until such text
    /// is corrected or removed, matching the original Linux XML tree.
    pub fn set_property_bytes(
        &mut self,
        image: i32,
        path: &[u8],
        value: Option<&[u8]>,
    ) -> Result<(), ParseError> {
        if path.is_empty()
            || !legal_path(path)
            || value.is_some_and(|v| {
                v.iter()
                    .any(|&c| c == 0 || (c < b' ' && !matches!(c, b'\t' | b'\n' | b'\r')))
            })
        {
            return Err(ParseError::InvalidParam);
        }
        let image_index = usize::try_from(image)
            .ok()
            .and_then(|i| i.checked_sub(1))
            .filter(|&i| i < self.images.len())
            .ok_or(ParseError::InvalidImage)?;
        if path == b"NAME"
            && value.is_some_and(|v| {
                !v.is_empty()
                    && (1..=self.images.len())
                        .any(|i| i != image as usize && self.name_bytes(i as i32) == Some(v))
            })
        {
            return Err(ParseError::ImageNameCollision);
        }
        let parsed = parse_path(path);
        let Child::Element(element) = &mut self.root.children[self.images[image_index]] else {
            return Err(ParseError::Xml);
        };
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            let parsed = match parsed {
                Ok(parsed) => parsed,
                Err(error) => {
                    for end in path
                        .iter()
                        .enumerate()
                        .filter_map(|(i, &c)| (c == b'/').then_some(i))
                    {
                        let prefix = parse_path(&path[..end])?;
                        element.ensure(&prefix)?;
                    }
                    return Err(error);
                }
            };
            let target = element.ensure(&parsed)?;
            target.attrs.clear();
            target.children.clear();
            target
                .children
                .try_push(Child::Text(copy_owned_bytes(value)?))
                .map_err(|_| ParseError::Nomem)?;
        } else if let Ok(path) = parsed {
            element.remove(&path);
        }
        Ok(())
    }
    /// Set or delete NAME, rejecting collisions with other images.
    pub fn set_name(&mut self, image: i32, value: Option<&str>) -> Result<(), ParseError> {
        self.set_property(image, "NAME", value)
    }
    /// Set or delete DESCRIPTION.
    pub fn set_description(&mut self, image: i32, value: Option<&str>) -> Result<(), ParseError> {
        self.set_property(image, "DESCRIPTION", value)
    }
    /// Set or delete FLAGS.
    pub fn set_flags(&mut self, image: i32, value: Option<&str>) -> Result<(), ParseError> {
        self.set_property(image, "FLAGS", value)
    }
    /// Serialize lossless platform bytes, with a UTF-8 BOM and upstream escaping.
    /// Arbitrary bytes in properties and element names remain unchanged.
    pub fn to_xml_bytes(&self) -> Vec<u8> {
        let mut output = b"\xef\xbb\xbf".to_vec();
        self.root.write(&mut output);
        output
    }
    /// Serialize as UTF-8, failing with InvalidUtf8 for arbitrary platform bytes.
    pub fn to_xml(&self) -> Result<String, ParseError> {
        String::from_utf8(self.to_xml_bytes()).map_err(|_| ParseError::InvalidUtf8String)
    }
    /// Serialize as UTF-16LE, accepting WTF-8 unpaired-surrogate codepoints.
    /// Arbitrary platform bytes outside WTF-8 fail with InvalidUtf8String.
    pub fn encode_utf16le(&self) -> Result<Vec<u8>, ParseError> {
        Ok(crate::platform_text::wtf8_to_utf16(&self.to_xml_bytes())?
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect())
    }
}
fn number(s: &str) -> u64 {
    let s = s.trim_start_matches([' ', '\t', '\r', '\n', '\x0b', '\x0c']);
    let (negative, s) = if let Some(s) = s.strip_prefix('-') {
        (true, s)
    } else {
        (false, s.strip_prefix('+').unwrap_or(s))
    };
    let Ok(v) = s.parse::<u64>() else { return 0 };
    let v = if negative { v.wrapping_neg() } else { v };
    if v == u64::MAX { 0 } else { v }
}
fn legal_path(path: &[u8]) -> bool {
    path.iter().enumerate().all(|(i, &c)| {
        c >= 128
            || c.is_ascii_alphabetic()
            || matches!(c, b':' | b'_')
            || matches!(c, b'/' | b'[' | b']')
            || (i > 0 && (c.is_ascii_digit() || matches!(c, b'-' | b'.')))
    })
}
/// Check the syntax of a property path without changing its XML tree.
pub fn valid_property_path_syntax(path: &[u8]) -> bool {
    parse_path(path).is_ok()
}
fn parse_path(path: &[u8]) -> Result<Vec<(&[u8], u32)>, ParseError> {
    if path.starts_with(b"/") || path.ends_with(b"/") {
        return Err(ParseError::InvalidParam);
    }
    let mut result = Vec::new();
    for component in path.split(|&c| c == b'/') {
        if component.is_empty() {
            return Err(ParseError::InvalidParam);
        }
        let (name, n) = if let Some(bracket) = component.iter().position(|&c| c == b'[') {
            let index = component[bracket + 1..]
                .strip_suffix(b"]")
                .ok_or(ParseError::InvalidParam)?;
            if index.is_empty() || !index.iter().all(u8::is_ascii_digit) {
                return Err(ParseError::InvalidParam);
            }
            let mut n = 0u32;
            for &digit in index {
                let next = n.wrapping_mul(10).wrapping_add((digit - b'0') as u32);
                if next < n {
                    return Err(ParseError::InvalidParam);
                }
                n = next;
            }
            (&component[..bracket], n)
        } else {
            (component, 1)
        };
        if name.is_empty() || n == 0 {
            return Err(ParseError::InvalidParam);
        }
        result.push((name, n));
    }
    Ok(result)
}
fn escape(text: &[u8], output: &mut Vec<u8>) {
    for &byte in text {
        match byte {
            b'<' => output.extend_from_slice(b"&lt;"),
            b'>' => output.extend_from_slice(b"&gt;"),
            b'&' => output.extend_from_slice(b"&amp;"),
            b'\'' => output.extend_from_slice(b"&apos;"),
            b'"' => output.extend_from_slice(b"&quot;"),
            _ => output.push(byte),
        }
    }
}
fn unescape(text: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut output = Vec::new();
    let mut remaining = text;
    while let Some(i) = remaining.iter().position(|&b| b == b'&') {
        output.extend_from_slice(&remaining[..i]);
        remaining = &remaining[i..];
        let mut matched = false;
        for (entity, byte) in [
            (b"&lt;".as_slice(), b'<'),
            (b"&gt;".as_slice(), b'>'),
            (b"&amp;".as_slice(), b'&'),
            (b"&apos;".as_slice(), b'\''),
            (b"&quot;".as_slice(), b'"'),
        ] {
            if let Some(rest) = remaining.strip_prefix(entity) {
                output.push(byte);
                remaining = rest;
                matched = true;
                break;
            }
        }
        if !matched {
            return Err(ParseError::Xml);
        }
    }
    output.extend_from_slice(remaining);
    Ok(output)
}
fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}
struct Parser<'a> {
    remaining: &'a [u8],
}
impl<'a> Parser<'a> {
    fn skip(&mut self, bytes: &[u8]) -> bool {
        if let Some(rest) = self.remaining.strip_prefix(bytes) {
            self.remaining = rest;
            true
        } else {
            false
        }
    }
    fn ws(&mut self) {
        while self.remaining.first().is_some_and(|&b| whitespace(b)) {
            self.remaining = &self.remaining[1..];
        }
    }
    fn until(&mut self, bytes: &[u8]) -> Result<&'a [u8], ParseError> {
        let i = self
            .remaining
            .windows(bytes.len())
            .position(|window| window == bytes)
            .ok_or(ParseError::Xml)?;
        let text = &self.remaining[..i];
        self.remaining = &self.remaining[i + bytes.len()..];
        Ok(text)
    }
    fn misc(&mut self) -> Result<(), ParseError> {
        loop {
            let previous = self.remaining.len();
            self.ws();
            if self.skip(b"<?") {
                self.until(b"?>")?;
            }
            if self.skip(b"<!DOCTYPE") {
                self.until(b">")?;
            }
            if self.skip(b"<!--") {
                self.until(b"-->")?;
            }
            if self.remaining.len() == previous {
                return Ok(());
            }
        }
    }
    fn element(&mut self, depth: usize) -> Result<Element, ParseError> {
        if depth >= 50 || !self.skip(b"<") {
            return Err(ParseError::Xml);
        }
        let n = self
            .remaining
            .iter()
            .position(|&b| whitespace(b) || matches!(b, b'>' | b'/'))
            .ok_or(ParseError::Xml)?;
        if n == 0 {
            return Err(ParseError::Xml);
        }
        let name = &self.remaining[..n];
        self.remaining = &self.remaining[n..];
        let mut element = Element::empty(name)?;
        while self.remaining.first().is_some_and(|&b| whitespace(b)) {
            self.ws();
            if self
                .remaining
                .first()
                .is_some_and(|b| matches!(b, b'>' | b'/'))
            {
                break;
            }
            let n = self
                .remaining
                .iter()
                .position(|&b| b == b'=' || whitespace(b))
                .ok_or(ParseError::Xml)?;
            if n == 0 {
                return Err(ParseError::Xml);
            }
            let attribute = &self.remaining[..n];
            self.remaining = &self.remaining[n..];
            self.ws();
            if !self.skip(b"=") {
                return Err(ParseError::Xml);
            }
            self.ws();
            let quote = if self.skip(b"\"") {
                b"\""
            } else if self.skip(b"'") {
                b"'"
            } else {
                return Err(ParseError::Xml);
            };
            let value = unescape(self.until(quote)?)?;
            element
                .attrs
                .try_push((copy_owned_bytes(attribute)?, copy_owned_bytes(&value)?))
                .map_err(|_| ParseError::Nomem)?;
        }
        if self.skip(b"/") {
            if !self.skip(b">") {
                return Err(ParseError::Xml);
            }
            return Ok(element);
        }
        if !self.skip(b">") {
            return Err(ParseError::Xml);
        }
        loop {
            let n = self
                .remaining
                .iter()
                .position(|&b| b == b'<')
                .ok_or(ParseError::Xml)?;
            element.append(unescape(&self.remaining[..n])?)?;
            self.remaining = &self.remaining[n..];
            if self.skip(b"</") {
                if !self.skip(name) {
                    return Err(ParseError::Xml);
                }
                self.ws();
                if !self.skip(b">") {
                    return Err(ParseError::Xml);
                }
                break;
            } else if self.skip(b"<?") {
                self.until(b"?>")?;
            } else if self.skip(b"<!--") {
                self.until(b"-->")?;
            } else if self.skip(b"<![CDATA[") {
                element.append(self.until(b"]]>")?.to_vec())?;
            } else {
                element
                    .children
                    .try_push(Child::Element(self.element(depth + 1)?))
                    .map_err(|_| ParseError::Nomem)?;
            }
        }
        Ok(element)
    }
}

fn copy_owned_bytes(bytes: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut result = Vec::new();
    result
        .try_extend_from_slice(bytes)
        .map_err(|_| ParseError::Nomem)?;
    Ok(result)
}
