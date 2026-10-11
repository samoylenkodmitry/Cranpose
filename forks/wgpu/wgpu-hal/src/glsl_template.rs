//! GLSL ES written ahead of time for one entry point, its pipeline constants
//! preprocessor macros. For a pipeline, the GL backend works out the
//! template's directives from the pipeline's constants and hands the browser
//! only the code they keep, as [`crate::auxil::prune`] would: no naga runs
//! when the pipeline is made.

use alloc::{borrow::Cow, format, string::String, vec::Vec};
use core::fmt::Write as _;

use hashbrown::HashMap;

/// How a pipeline constant's value is written as its macro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlslMacroKind {
    Bool,
    Sint,
    Uint,
    Float,
}

/// A pipeline constant a template reads as a macro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlslMacro {
    /// The constant's name among the pipeline's constants.
    pub constant: &'static str,
    /// The macro's name in the template.
    pub name: &'static str,
    pub kind: GlslMacroKind,
}

/// A uniform or storage block, by the name the template gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlslBlock {
    pub name: &'static str,
    pub storage: bool,
    pub binding: naga::ResourceBinding,
}

/// A texture, by the name the template gives it, with the sampler GLSL ES
/// combines it with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlslTexture {
    pub name: &'static str,
    pub storage: bool,
    pub binding: naga::ResourceBinding,
    pub sampler: Option<naga::ResourceBinding>,
}

/// GLSL ES of one entry point whose pipeline constants are macros, and the
/// names the backend binds its resources by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlslTemplate {
    pub entry_point: &'static str,
    pub stage: naga::ShaderStage,
    /// The GLSL, in pieces that templates share: the first is the
    /// `#version` line. Each macro's default is defined under `#ifndef`, so
    /// a definition in front of it wins.
    pub glsl: &'static [&'static str],
    pub macros: &'static [GlslMacro],
    pub blocks: &'static [GlslBlock],
    pub textures: &'static [GlslTexture],
    /// Each vertex input or fragment output, by name, with its location.
    pub varyings: &'static [(&'static str, u32)],
    pub clip_distance_count: u32,
}

impl GlslTemplate {
    /// The template's GLSL for a pipeline whose constants are `constants`:
    /// its directives worked out here, so the browser gets only the code the
    /// constants keep, with each constant the code reads defined after the
    /// `#version` line. Errors on a value a macro cannot hold and on a
    /// directive the template should not have.
    pub fn expand(&self, constants: &naga::back::PipelineConstants) -> Result<String, String> {
        let mut macros = Macros::default();
        for item in self.macros {
            if let Some(&value) = constants.get(item.constant) {
                macros.define(item.name, Cow::Owned(macro_value(item, value)?));
            }
        }
        let length = self.glsl.iter().map(|piece| piece.len()).sum::<usize>();
        let mut code = String::with_capacity(length);
        let mut version = "";
        // Whether the lines of each open conditional are kept.
        let mut kept: Vec<bool> = Vec::new();
        for line in self.glsl.iter().flat_map(|piece| piece.split_inclusive('\n')) {
            let keeping = kept.last().copied().unwrap_or(true);
            let Some(directive) = line.trim_start().strip_prefix('#') else {
                if keeping {
                    code.push_str(line);
                }
                continue;
            };
            let directive = directive.trim_end();
            let (name, rest) = directive.split_once(' ').unwrap_or((directive, ""));
            match name {
                "version" => version = line,
                "ifndef" => kept.push(keeping && !macros.defined(rest.trim())),
                "if" => {
                    let holds = keeping && macros.evaluate(rest)? != 0;
                    kept.push(holds);
                }
                "endif" => {
                    kept.pop().ok_or("#endif without #if")?;
                }
                "define" if keeping => {
                    let (macro_name, value) = rest.split_once(' ').unwrap_or((rest, ""));
                    macros.define(macro_name, Cow::Borrowed(value));
                }
                "define" => {}
                other => return Err(format!("#{other} in a GLSL template")),
            }
        }
        if !kept.is_empty() {
            return Err("a GLSL template leaves a conditional open".into());
        }
        let mut glsl = String::with_capacity(version.len() + 48 * self.macros.len() + code.len());
        glsl.push_str(version);
        for item in self.macros {
            if let Some(value) = macros.values.get(item.name) {
                let _ = writeln!(glsl, "#define {} {value}", item.name);
            }
        }
        glsl.push_str(&code);
        Ok(glsl)
    }
}

/// A pipeline constant's value as its macro: a bool as 1 or 0, an integer
/// without a suffix, which the preprocessor reads, and a float with its
/// point.
fn macro_value(item: &GlslMacro, value: f64) -> Result<String, String> {
    let unfit = || format!("{}: {value} is no {:?}", item.constant, item.kind);
    Ok(match item.kind {
        GlslMacroKind::Bool => format!("{}", u8::from(value != 0.0)),
        GlslMacroKind::Sint => {
            if value % 1.0 != 0.0
                || !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value)
            {
                return Err(unfit());
            }
            format!("{}", value as i32)
        }
        // An integer literal is signed: a value past `i32::MAX` is written
        // as its bit pattern.
        GlslMacroKind::Uint => {
            if value % 1.0 != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&value) {
                return Err(unfit());
            }
            let value = value as u32;
            if value > i32::MAX as u32 {
                format!("{value:#X}")
            } else {
                format!("{value}")
            }
        }
        GlslMacroKind::Float => {
            if !value.is_finite() {
                return Err(unfit());
            }
            format!("{:?}", value as f32)
        }
    })
}

/// The macros a template defines, and the integer value of each its
/// directives read.
#[derive(Default)]
struct Macros<'t> {
    values: HashMap<&'t str, Cow<'t, str>>,
    evaluated: HashMap<&'t str, i64>,
}

impl<'t> Macros<'t> {
    /// Defines `name` unless it is: a pipeline's value comes first.
    fn define(&mut self, name: &'t str, value: Cow<'t, str>) {
        self.values.entry(name).or_insert(value);
    }

    fn defined(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// The value of the `#if` expression `text`.
    fn evaluate(&mut self, text: &str) -> Result<i64, String> {
        Evaluator {
            values: &self.values,
            evaluated: &mut self.evaluated,
        }
        .evaluate(text)
    }
}

/// Works out expressions over macros, each macro's value once.
struct Evaluator<'a, 't> {
    values: &'a HashMap<&'t str, Cow<'t, str>>,
    evaluated: &'a mut HashMap<&'t str, i64>,
}

impl Evaluator<'_, '_> {
    fn evaluate(&mut self, text: &str) -> Result<i64, String> {
        let mut parser = Parser { text, at: 0 };
        let value = parser.binary(self, 0)?;
        parser.skip_space();
        if parser.at != text.len() {
            return Err(format!("`{text}` does not end at {}", parser.at));
        }
        Ok(value)
    }

    fn value(&mut self, name: &str) -> Result<i64, String> {
        if let Some(&value) = self.evaluated.get(name) {
            return Ok(value);
        }
        let values = self.values;
        let (&key, text) = values
            .get_key_value(name)
            .ok_or_else(|| format!("`{name}` is not defined"))?;
        let value = self.evaluate(text)?;
        self.evaluated.insert(key, value);
        Ok(value)
    }
}

/// Binary operators by precedence, loosest first, as C's.
const LEVELS: [&[&str]; 10] = [
    &["||"],
    &["&&"],
    &["|"],
    &["^"],
    &["&"],
    &["==", "!="],
    &["<=", ">=", "<", ">"],
    &["<<", ">>"],
    &["+", "-"],
    &["*", "/", "%"],
];

struct Parser<'s> {
    text: &'s str,
    at: usize,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while self.text[self.at..].starts_with([' ', '\t']) {
            self.at += 1;
        }
    }

    /// The operator of `level` at the cursor, taken.
    fn operator(&mut self, level: usize) -> Option<&'static str> {
        self.skip_space();
        let rest = &self.text[self.at..];
        let found = LEVELS[level].iter().copied().find(|operator| {
            rest.starts_with(operator)
                // `&` and `|` are not the first half of `&&` and `||`, nor
                // `<` of `<<`.
                && !(operator.len() == 1 && rest[1..].starts_with(*operator))
        })?;
        self.at += found.len();
        Some(found)
    }

    fn binary(&mut self, macros: &mut Evaluator<'_, '_>, level: usize) -> Result<i64, String> {
        if level == LEVELS.len() {
            return self.unary(macros);
        }
        let mut value = self.binary(macros, level + 1)?;
        while let Some(operator) = self.operator(level) {
            let right = self.binary(macros, level + 1)?;
            value = match operator {
                "||" => i64::from(value != 0 || right != 0),
                "&&" => i64::from(value != 0 && right != 0),
                "|" => value | right,
                "^" => value ^ right,
                "&" => value & right,
                "==" => i64::from(value == right),
                "!=" => i64::from(value != right),
                "<" => i64::from(value < right),
                ">" => i64::from(value > right),
                "<=" => i64::from(value <= right),
                ">=" => i64::from(value >= right),
                "<<" => value.wrapping_shl(right as u32),
                ">>" => value.wrapping_shr(right as u32),
                "+" => value.wrapping_add(right),
                "-" => value.wrapping_sub(right),
                "*" => value.wrapping_mul(right),
                "/" => value.checked_div(right).ok_or("division by zero")?,
                _ => value.checked_rem(right).ok_or("division by zero")?,
            };
        }
        Ok(value)
    }

    fn unary(&mut self, macros: &mut Evaluator<'_, '_>) -> Result<i64, String> {
        self.skip_space();
        let rest = &self.text[self.at..];
        let Some(first) = rest.chars().next() else {
            return Err(format!("`{}` ends where a value goes", self.text));
        };
        match first {
            '!' | '~' | '-' | '+' => {
                self.at += 1;
                let value = self.unary(macros)?;
                Ok(match first {
                    '!' => i64::from(value == 0),
                    '~' => !value,
                    '-' => value.wrapping_neg(),
                    _ => value,
                })
            }
            '(' => {
                self.at += 1;
                let value = self.binary(macros, 0)?;
                self.skip_space();
                if !self.text[self.at..].starts_with(')') {
                    return Err(format!("`{}` leaves a parenthesis open", self.text));
                }
                self.at += 1;
                Ok(value)
            }
            _ => {
                let length = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                let word = &rest[..length];
                self.at += length;
                if first.is_ascii_digit() {
                    let (digits, radix) = match word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
                        Some(hex) => (hex, 16),
                        None => (word, 10),
                    };
                    i64::from_str_radix(digits.trim_end_matches(['u', 'U']), radix)
                        .map_err(|error| format!("`{word}`: {error}"))
                } else if word.is_empty() {
                    Err(format!("`{}` has no value at {}", self.text, self.at))
                } else {
                    macros.value(word)
                }
            }
        }
    }
}
