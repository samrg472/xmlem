use std::{
    borrow::Cow,
    fmt::Display,
    io::{self, Write},
};

use indexmap::IndexMap;

use crate::{
    document::{Declaration, Document},
    key::DocKey,
    value::{ElementValue, NodeValue},
};

pub(crate) trait Print<Config, Context = ()> {
    fn print(&self, f: &mut dyn Write, config: &Config, context: &Context) -> std::io::Result<()>;
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub is_pretty: bool,
    pub indent: usize,
    pub max_line_length: usize,
    pub entity_mode: EntityMode,
}

impl Config {
    pub fn default_pretty() -> Self {
        Config {
            is_pretty: true,
            indent: 2,
            max_line_length: 120,
            entity_mode: EntityMode::Standard,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct State<'a> {
    pub indent: usize,
    pub key: DocKey,
    pub doc: &'a Document,
}

impl<'a> State<'a> {
    pub(crate) fn new(document: &'a Document) -> Self {
        Self {
            indent: 0,
            doc: document,
            key: document.root_key.0,
        }
    }

    fn with_indent(&self, config: &Config) -> Self {
        if !config.is_pretty {
            return self.clone();
        }

        State {
            indent: self.indent + config.indent,
            key: self.key,
            doc: self.doc,
        }
    }

    fn with_key(&self, key: DocKey) -> Self {
        State {
            indent: self.indent,
            key,
            doc: self.doc,
        }
    }
}

impl Print<Config, State<'_>> for Declaration {
    fn print(
        &self,
        f: &mut dyn Write,
        config: &Config,
        _context: &State<'_>,
    ) -> std::io::Result<()> {
        write!(f, "<?xml ")?;

        if let Some(version) = self.version.as_deref() {
            write!(f, "version=\"{}\" ", version)?;
        }

        if let Some(encoding) = self.encoding.as_deref() {
            write!(f, "encoding=\"{}\" ", encoding)?;
        }

        if let Some(standalone) = self.standalone.as_deref() {
            write!(f, "standalone=\"{}\" ", standalone)?;
        }

        write!(f, "?>")?;

        if config.is_pretty {
            writeln!(f)?;
        }

        Ok(())
    }
}

impl Display for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut config = if f.alternate() {
            Config::default_pretty()
        } else {
            Config::default()
        };

        if let Some(width) = f.width() {
            config.is_pretty = true;
            config.indent = width;
        }

        if let Some(precision) = f.precision() {
            config.is_pretty = true;
            config.max_line_length = precision;
        }

        self.print(&mut FmtWriter(f), &config, &State::new(self))
            .map_err(|_| std::fmt::Error)
    }
}

impl Print<Config, State<'_>> for Document {
    fn print(
        &self,
        f: &mut dyn Write,
        config: &Config,
        context: &State<'_>,
    ) -> std::io::Result<()> {
        if let Some(decl) = self.decl.as_ref() {
            Print::print(decl, f, &config, &context)?;
        }

        for node in self.before.iter() {
            let node_value = self.nodes.get(node.as_key()).unwrap();
            node_value.print(f, config, &context.with_key(node.as_key()))?;
        }

        let element = self
            .nodes
            .get(self.root_key.0)
            .unwrap()
            .as_element()
            .unwrap();

        element.print(f, config, &context.with_key(self.root_key.0))?;

        for node in self.after.iter() {
            let node_value = self.nodes.get(node.as_key()).unwrap();
            node_value.print(f, config, &context.with_key(node.as_key()))?;
        }

        Ok(())
    }
}

fn fmt_attrs(
    f: &mut dyn Write,
    tag: &str,
    config: &Config,
    context: &State,
    attrs: &IndexMap<String, String>,
) -> io::Result<()> {
    let line_length = tag.len()
        + 2
        + attrs
            .iter()
            .fold(0usize, |acc, (k, v)| acc + k.len() + v.len() + 4);

    let is_newlines = config.is_pretty && line_length > config.max_line_length;
    let context = context.with_indent(config);

    let mut iter = attrs.iter();

    if let Some((k, v)) = iter.next() {
        if is_newlines {
            writeln!(f)?;
            write!(f, "{:>indent$}", "", indent = context.indent)?;
        }
        write!(f, "{}=\"{}\"", k, process_entities(v, config.entity_mode))?;
    } else {
        return Ok(());
    }

    for (k, v) in iter {
        if is_newlines {
            writeln!(f)?;
            write!(f, "{:>indent$}", "", indent = context.indent)?;
        } else {
            write!(f, " ")?;
        }
        write!(f, "{}=\"{}\"", k, process_entities(v, config.entity_mode))?;
    }

    Ok(())
}

impl Print<Config, State<'_>> for ElementValue {
    fn print(
        &self,
        f: &mut dyn Write,
        config: &Config,
        context: &State<'_>,
    ) -> std::io::Result<()> {
        if self.children.is_empty() {
            match context.doc.attrs.get(context.key) {
                Some(attrs) if !attrs.is_empty() => {
                    write!(f, "{:>indent$}<{} ", "", self.name, indent = context.indent)?;
                    fmt_attrs(f, &self.name, config, context, attrs)?;
                    write!(f, " />")?;
                    if config.is_pretty {
                        writeln!(f)?;
                    }
                    return Ok(());
                }
                _ => {
                    write!(
                        f,
                        "{:>indent$}<{} />",
                        "",
                        self.name,
                        indent = context.indent
                    )?;
                    if config.is_pretty {
                        writeln!(f)?;
                    }
                    return Ok(());
                }
            }
        }

        match context.doc.attrs.get(context.key) {
            Some(attrs) if !attrs.is_empty() => {
                write!(f, "{:>indent$}<{} ", "", self.name, indent = context.indent)?;
                fmt_attrs(f, &self.name, config, context, attrs)?;
                write!(f, ">")?;
                if config.is_pretty {
                    writeln!(f)?;
                }
            }
            _ => {
                write!(f, "{:>indent$}<{}>", "", self.name, indent = context.indent)?;
                if config.is_pretty {
                    writeln!(f)?;
                }
            }
        }

        let child_context = context.with_indent(config);

        for child in self.children.iter() {
            let value = context.doc.nodes.get(child.as_key()).unwrap();
            value.print(f, config, &child_context.with_key(child.as_key()))?;
        }
        write!(
            f,
            "{:>indent$}</{}>",
            "",
            self.name,
            indent = context.indent
        )?;

        if config.is_pretty {
            writeln!(f)?;
        }

        Ok(())
    }
}

impl Print<Config, State<'_>> for NodeValue {
    fn print(
        &self,
        f: &mut dyn Write,
        config: &Config,
        context: &State<'_>,
    ) -> std::io::Result<()> {
        if let NodeValue::Element(e) = self {
            return e.print(f, config, context);
        }

        if config.is_pretty {
            write!(f, "{:>indent$}", "", indent = context.indent)?;
        }

        match self {
            NodeValue::Text(t) => write!(f, "{}", &*process_entities(t, config.entity_mode).trim()),
            NodeValue::CData(t) => write!(f, "<![CDATA[{}]]>", t),
            NodeValue::DocumentType(t) => write!(f, "<!DOCTYPE{}>", t),
            NodeValue::Comment(t) => write!(f, "<!--{}-->", t),
            NodeValue::Element(_) => unreachable!(),
        }?;

        if config.is_pretty {
            writeln!(f)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityMode {
    Standard,
    Hex,
}

impl Default for EntityMode {
    fn default() -> Self {
        Self::Standard
    }
}

fn process_entities(input: &str, mode: EntityMode) -> Cow<'_, str> {
    if input.contains(['<', '>', '\'', '"', '&']) || input.contains(|c: char| c.is_ascii_control())
    {
        let mut s = String::with_capacity(input.len());
        input.chars().for_each(|ch| {
            s.push_str(match (mode, ch) {
                (EntityMode::Standard, '&') => "&amp;",
                (EntityMode::Standard, '\'') => "&apos;",
                (EntityMode::Standard, '"') => "&quot;",
                (EntityMode::Standard, '<') => "&lt;",
                (EntityMode::Standard, '>') => "&gt;",
                (EntityMode::Hex, '&' | '\'' | '"' | '<' | '>') => {
                    s.push_str(&format!("&#x{:>04X};", ch as u32));
                    return;
                }
                (_, ch) if ch.is_ascii_control() => {
                    s.push_str(&format!("&#x{:>04X};", ch as u32));
                    return;
                }
                (_, other) => {
                    s.push(other);
                    return;
                }
            })
        });
        Cow::Owned(s)
    } else {
        Cow::Borrowed(input)
    }
}

struct FmtWriter<'a, 'b>(&'b mut std::fmt::Formatter<'a>);

impl Write for FmtWriter<'_, '_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let s = std::str::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.0
            .write_str(s)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(s.as_bytes().len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
