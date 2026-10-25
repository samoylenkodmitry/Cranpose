//! Writes a template from WGSL, in a build script.
//!
//! The entry point's module is compacted to what the entry point reaches and
//! validated. Each side of an `if` that the pipeline constants can rule out
//! goes under a preprocessor `#if` that holds for the constants that leave
//! the side reachable, and each function under one that holds where a call
//! to it is left. The `if` itself stays, its condition reading the constants
//! as macros, so a side the constants rule out reaches the compiler empty.
//! What the constants decide is worked out as naga's pruning does it
//! (`wgpu_hal::auxil::prune`): integer and bool operations, `select`,
//! `min`, `max`, and calls of functions whose first statement returns a
//! value. A condition joins a constant to a run-time value with `&`: naga
//! writes WGSL's `&&` with a run-time side as a branch of its own.
//!
//! naga's GLSL writer neither writes directives nor reads unresolved
//! constants, so the generator marks each directive as a call to an empty
//! function, and each constant as a numbered literal, and puts the directives
//! and the macros in their place in the written text.

use std::{collections::HashMap, fmt::Write as _};

use naga::{
    Arena, BinaryOperator, Block, Expression, Function, Handle, Literal, MathFunction, Module,
    Scalar, ScalarKind, Span, Statement, TypeInner, UnaryOperator, back::glsl, valid,
};

/// The literal each pipeline constant is written as before it becomes its
/// macro: this plus the constant's index, ten digits long.
const SENTINEL: u32 = 0xC0DE_0000;
/// What a directive's marker function is named, around its index.
const MARKER: (&str, &str) = ("cranposepp", "x");
/// What a directive's text starts with when it puts the function it opens
/// under the condition after it.
const FUNCTION_GUARD: &str = "#function ";

/// How a pipeline constant's value is written as its macro: the variants of
/// `wgpu_hal::GlslMacroKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacroKind {
    /// A `bool` constant, written as `0` or `1`.
    Bool,
    /// An `i32` constant.
    Sint,
    /// A `u32` constant.
    Uint,
    /// An `f32` constant.
    Float,
}

/// The template of one entry point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    /// The entry point's name in the WGSL.
    pub entry_point: String,
    /// The pipeline stage the entry point runs in.
    pub stage: naga::ShaderStage,
    /// The GLSL of the entry point and what a backend needs to bind it.
    pub generated: Generated,
}

/// The templates of every vertex and fragment entry point of the WGSL
/// `source`.
pub fn module_templates(source: &str) -> Result<Vec<Template>, String> {
    let module =
        naga::front::wgsl::parse_str(source).map_err(|error| error.emit_to_string(source))?;
    module
        .entry_points
        .iter()
        .filter(|entry| {
            matches!(
                entry.stage,
                naga::ShaderStage::Vertex | naga::ShaderStage::Fragment
            )
        })
        .map(|entry| {
            Ok(Template {
                entry_point: entry.name.clone(),
                stage: entry.stage,
                generated: generate(source, &entry.name, entry.stage)?,
            })
        })
        .collect()
}

/// A uniform or storage block, by the name the template gives it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockBinding {
    /// The block's name in the GLSL.
    pub name: String,
    /// Whether the block is a storage block instead of a uniform block.
    pub storage: bool,
    /// The WGSL group and binding of the block.
    pub binding: naga::ResourceBinding,
}

/// A texture, by the name the template gives it, with the sampler GLSL ES
/// combines it with.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureBinding {
    /// The texture's name in the GLSL.
    pub name: String,
    /// Whether the texture is a storage texture instead of a sampled one.
    pub storage: bool,
    /// The WGSL group and binding of the texture.
    pub binding: naga::ResourceBinding,
    /// The WGSL group and binding of the sampler combined with the texture, or `None`
    /// when no sampler is.
    pub sampler: Option<naga::ResourceBinding>,
}

/// A template as the build script writes it into the renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generated {
    /// The GLSL ES 3.00 text, starting with its `#version` line, with the pipeline
    /// constants as macros and the branches they can rule out under `#if`.
    pub glsl: String,
    /// The pipeline constants a pipeline sets, each by its WGSL name, with how its
    /// value is written as a macro.
    pub macros: Vec<(String, MacroKind)>,
    /// The uniform and storage blocks the GLSL names, sorted.
    pub blocks: Vec<BlockBinding>,
    /// The textures the GLSL names, sorted.
    pub textures: Vec<TextureBinding>,
    /// Each vertex input or fragment output by name, with its location, sorted by name.
    pub varyings: Vec<(String, u32)>,
    /// How many clip distances the vertex stage writes; zero when it writes none.
    pub clip_distance_count: u32,
}

/// The template of `entry_point`, the `naga_stage` entry point of the WGSL
/// `source`, for WebGL2.
pub fn generate(
    source: &str,
    entry_point: &str,
    naga_stage: naga::ShaderStage,
) -> Result<Generated, String> {
    let (mut module, root) = stage_module(source, entry_point, naga_stage)?;
    let original = module.clone();
    let analysis = Analysis { module: &original };
    let mut definitions = Definitions::default();
    let (texts, reach) = directives(&analysis, root, &mut definitions);
    let markers: HashMap<String, Handle<Function>> = texts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let handle = module.functions.append(marker(index), Span::UNDEFINED);
            (text.clone(), handle)
        })
        .collect();
    let info = validate(&module)?;
    rewrite(&mut module, &analysis, &reach, &markers, &mut definitions);
    let macros = macros(&module)?;
    mark_constants(&mut module).map_err(|error| format!("{entry_point}: {error}"))?;
    let (written, reflection) = write_glsl(&module, &info, entry_point, naga_stage)?;
    if !reflection.immediates_items.is_empty() {
        return Err(format!("{entry_point}: immediates are not templated"));
    }
    let (blocks, textures) = bindings(&module, &reflection);
    let mut varyings: Vec<(String, u32)> = reflection
        .varying
        .iter()
        .map(|(name, location)| (name.clone(), location.location))
        .collect();
    varyings.sort();
    Ok(Generated {
        glsl: finish(&written, &texts, &macros, &definitions)?,
        macros: macros
            .iter()
            .filter(|constant| constant.settable)
            .map(|constant| (constant.name.clone(), constant.kind))
            .collect(),
        blocks,
        textures,
        varyings,
        clip_distance_count: reflection.clip_distance_count,
    })
}

/// The WGSL `source` with its `naga_stage` entry points alone, compacted and
/// validated, and the index of `entry_point` among them. Every entry point
/// of the stage stays, so naga names each function and local alike in the
/// templates of all of them, and they share that code.
fn stage_module(
    source: &str,
    entry_point: &str,
    naga_stage: naga::ShaderStage,
) -> Result<(Module, usize), String> {
    let mut module =
        naga::front::wgsl::parse_str(source).map_err(|error| error.emit_to_string(source))?;
    module
        .entry_points
        .retain(|entry| entry.stage == naga_stage);
    let root = module
        .entry_points
        .iter()
        .position(|entry| entry.name == entry_point)
        .ok_or_else(|| format!("no {naga_stage:?} entry point {entry_point}"))?;
    naga::compact::compact(&mut module, naga::compact::KeepUnused::No);
    validate(&module)?;
    Ok((module, root))
}

/// The text of each directive the module's bodies take, and of each
/// function's condition, in the order their markers are made; and the
/// condition under which the entry point `root` calls each function.
fn directives(
    analysis: &Analysis<'_>,
    root: usize,
    definitions: &mut Definitions,
) -> (Vec<String>, Vec<String>) {
    let mut collected = Directives::Collect(Vec::new());
    let calls: Vec<Vec<(Handle<Function>, String)>> = bodies(analysis.module)
        .map(|function| {
            let mut pass = Transform::new(analysis, function, &mut collected, definitions);
            pass.block(&function.body);
            pass.calls
        })
        .collect();
    let reach = reachability(analysis.module, &calls, root, definitions);
    let mut texts = match collected {
        Directives::Collect(texts) => texts,
        Directives::Apply(_) => Vec::new(),
    };
    for when in &reach {
        let text = format!("{FUNCTION_GUARD}{when}");
        if when != "1" && !texts.contains(&text) {
            texts.push(text);
        }
    }
    (texts, reach)
}

/// Puts the directives in `module`'s bodies as calls of their `markers`,
/// and each function under the condition `reach` gives it.
fn rewrite(
    module: &mut Module,
    analysis: &Analysis<'_>,
    reach: &[String],
    markers: &HashMap<String, Handle<Function>>,
    definitions: &mut Definitions,
) {
    let mut apply = Directives::Apply(markers);
    let bodies: Vec<Block> = bodies(analysis.module)
        .map(|function| {
            Transform::new(analysis, function, &mut apply, definitions).block(&function.body)
        })
        .collect();
    let mut rewritten = bodies.into_iter().enumerate().map(|(index, body)| {
        match reach.get(index).filter(|when| *when != "1") {
            Some(when) => {
                let mut guarded = Block::with_capacity(body.len() + 1);
                guarded.push(
                    apply.directive(format!("{FUNCTION_GUARD}{when}")),
                    Span::UNDEFINED,
                );
                for (statement, span) in body.span_into_iter() {
                    guarded.push(statement, span);
                }
                guarded
            }
            None => body,
        }
    });
    let functions = module
        .functions
        .iter_mut()
        .map(|(_, function)| function)
        .filter(|function| !function.name.as_deref().is_some_and(is_marker));
    let entries = module
        .entry_points
        .iter_mut()
        .map(|entry| &mut entry.function);
    for function in functions.chain(entries) {
        if let Some(body) = rewritten.next() {
            function.body = body;
        }
    }
}

/// `module` as GLSL ES 3.00 for WebGL with `entry_point` its `main`, as the
/// GL backend's naga options write it, and what the writer tells of it.
fn write_glsl(
    module: &Module,
    info: &valid::ModuleInfo,
    entry_point: &str,
    naga_stage: naga::ShaderStage,
) -> Result<(String, glsl::ReflectionInfo), String> {
    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        writer_flags: glsl::WriterFlags::ADJUST_COORDINATE_SPACE,
        binding_map: Default::default(),
        zero_initialize_workgroup_memory: true,
    };
    let pipeline_options = glsl::PipelineOptions {
        shader_stage: naga_stage,
        entry_point: entry_point.to_string(),
        multiview: None,
    };
    let unchecked = naga::proc::BoundsCheckPolicy::Unchecked;
    let policies = naga::proc::BoundsCheckPolicies {
        index: unchecked,
        buffer: unchecked,
        image_load: unchecked,
        binding_array: unchecked,
    };
    let mut written = String::new();
    let reflection = glsl::Writer::new(
        &mut written,
        module,
        info,
        &options,
        &pipeline_options,
        policies,
    )
    .and_then(|mut writer| writer.write())
    .map_err(|error| format!("{entry_point}: {error}"))?;
    Ok((written, reflection))
}

/// The blocks and textures the written GLSL names, in a stable order.
fn bindings(
    module: &Module,
    reflection: &glsl::ReflectionInfo,
) -> (Vec<BlockBinding>, Vec<TextureBinding>) {
    let binding = |handle: Handle<naga::GlobalVariable>| module.global_variables[handle].binding;
    let mut blocks: Vec<BlockBinding> = reflection
        .uniforms
        .iter()
        .filter_map(|(handle, name)| {
            Some(BlockBinding {
                name: name.clone(),
                storage: matches!(
                    module.global_variables[*handle].space,
                    naga::AddressSpace::Storage { .. }
                ),
                binding: binding(*handle)?,
            })
        })
        .collect();
    blocks.sort();
    let mut textures: Vec<TextureBinding> = reflection
        .texture_mapping
        .iter()
        .filter_map(|(name, mapping)| {
            Some(TextureBinding {
                name: name.clone(),
                storage: matches!(
                    module.types[module.global_variables[mapping.texture].ty].inner,
                    TypeInner::Image {
                        class: naga::ImageClass::Storage { .. },
                        ..
                    }
                ),
                binding: binding(mapping.texture)?,
                sampler: mapping.sampler.and_then(binding),
            })
        })
        .collect();
    textures.sort();
    (blocks, textures)
}

fn validate(module: &Module) -> Result<valid::ModuleInfo, String> {
    valid::Validator::new(valid::ValidationFlags::all(), valid::Capabilities::all())
        .validate(module)
        .map_err(|error| format!("{error:?}"))
}

/// The module's function bodies, functions first, then the entry points,
/// leaving the marker functions out.
fn bodies(module: &Module) -> impl Iterator<Item = &Function> {
    module
        .functions
        .iter()
        .map(|(_, function)| function)
        .filter(|function| !function.name.as_deref().is_some_and(is_marker))
        .chain(module.entry_points.iter().map(|entry| &entry.function))
}

fn is_marker(name: &str) -> bool {
    name.starts_with(MARKER.0) && name.ends_with(MARKER.1)
}

/// An empty function a directive's place in the text is a call of.
fn marker(index: usize) -> Function {
    Function {
        name: Some(format!("{}{index}{}", MARKER.0, MARKER.1)),
        arguments: Vec::new(),
        result: None,
        local_variables: Arena::new(),
        expressions: Arena::new(),
        named_expressions: Default::default(),
        body: Block::from_vec(vec![Statement::Return { value: None }]),
        diagnostic_filter_leaf: None,
    }
}

/// Where the transform puts its directives: into a list of their texts, or
/// as calls of their markers.
enum Directives<'a> {
    Collect(Vec<String>),
    Apply(&'a HashMap<String, Handle<Function>>),
}

impl Directives<'_> {
    fn directive(&mut self, text: String) -> Statement {
        match self {
            Self::Collect(texts) => {
                if !texts.contains(&text) {
                    texts.push(text);
                }
                Statement::Kill
            }
            Self::Apply(markers) => match markers.get(&text) {
                Some(function) => Statement::Call {
                    function: *function,
                    arguments: Vec::new(),
                    result: None,
                },
                None => Statement::Kill,
            },
        }
    }
}

/// The module before the transform: what the conditions read.
struct Analysis<'m> {
    module: &'m Module,
}

impl Analysis<'_> {
    /// `expression` as a preprocessor expression over the macros, when the
    /// preprocessor can read it: integers and bools, which it reads as 1 or 0.
    fn text(&self, arena: &Arena<Expression>, expression: Handle<Expression>) -> Option<String> {
        let globals = &self.module.global_expressions;
        Some(match arena[expression] {
            Expression::Literal(literal) => literal_text(literal)?,
            Expression::Constant(constant) => {
                self.text(globals, self.module.constants[constant].init)?
            }
            Expression::Override(constant) => {
                format!("CP_{}", self.module.overrides[constant].name.as_deref()?)
            }
            Expression::Unary { op, expr } => {
                unary(op, self.is_bool(arena, expr), &self.text(arena, expr)?)
            }
            Expression::Binary { op, left, right } => format!(
                "({} {} {})",
                self.text(arena, left)?,
                symbol(op, self.is_bool(arena, left))?,
                self.text(arena, right)?
            ),
            Expression::As {
                expr,
                kind: ScalarKind::Sint | ScalarKind::Uint,
                ..
            } => self.text(arena, expr)?,
            Expression::As {
                expr,
                kind: ScalarKind::Bool,
                ..
            } => format!("({} != 0)", self.text(arena, expr)?),
            _ => return None,
        })
    }

    /// `expression`, a constants-only value, as GLSL code: what a pipeline
    /// constant's use in the shader becomes. A constant the pipeline sets is
    /// its macro, cast to its type; one the shader derives from others is
    /// its definition, so its macro, an integer for the preprocessor, never
    /// reaches the code.
    fn code(&self, arena: &Arena<Expression>, expression: Handle<Expression>) -> Option<String> {
        let globals = &self.module.global_expressions;
        Some(match arena[expression] {
            Expression::Literal(literal) => literal_code(literal)?,
            Expression::Constant(constant) => {
                self.code(globals, self.module.constants[constant].init)?
            }
            Expression::Override(constant) => self.override_code(constant)?,
            Expression::Unary { op, expr } => {
                unary(op, self.is_bool(arena, expr), &self.code(arena, expr)?)
            }
            Expression::Binary { op, left, right } => format!(
                "({} {} {})",
                self.code(arena, left)?,
                symbol(op, self.is_bool(arena, left))?,
                self.code(arena, right)?
            ),
            Expression::As { expr, kind, .. } => {
                format!("{}({})", cast(kind)?, self.code(arena, expr)?)
            }
            _ => return None,
        })
    }

    /// A pipeline constant's use in code: the macro of one the pipeline sets,
    /// cast to its type; the definition of one the shader derives.
    fn override_code(&self, constant: Handle<naga::Override>) -> Option<String> {
        let found = &self.module.overrides[constant];
        let globals = &self.module.global_expressions;
        if let Some(init) = found.init
            && !matches!(globals[init], Expression::Literal(_))
        {
            return self.code(globals, init);
        }
        let TypeInner::Scalar(Scalar { kind, .. }) = self.module.types[found.ty].inner else {
            return None;
        };
        Some(format!("{}(CP_{})", cast(kind)?, found.name.as_deref()?))
    }

    /// Whether `expression` is a bool, as the global expressions and the
    /// function's own resolve it.
    fn is_bool(&self, arena: &Arena<Expression>, expression: Handle<Expression>) -> bool {
        match arena[expression] {
            Expression::Literal(Literal::Bool(_)) => true,
            Expression::Override(constant) => {
                is_bool_type(self.module, self.module.overrides[constant].ty)
            }
            Expression::Constant(constant) => {
                is_bool_type(self.module, self.module.constants[constant].ty)
            }
            Expression::Unary {
                op: UnaryOperator::LogicalNot,
                ..
            } => true,
            Expression::Unary { expr, .. } => self.is_bool(arena, expr),
            Expression::Binary { op, left, .. } => match op {
                BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::Less
                | BinaryOperator::LessEqual
                | BinaryOperator::Greater
                | BinaryOperator::GreaterEqual
                | BinaryOperator::LogicalAnd
                | BinaryOperator::LogicalOr => true,
                _ => self.is_bool(arena, left),
            },
            Expression::As { kind, .. } => kind == ScalarKind::Bool,
            _ => false,
        }
    }

    /// The function a call result comes from, when that function's first
    /// statement returns a value: what the call evaluates to.
    fn returned(&self, callee: Handle<Function>) -> Option<(&Function, Handle<Expression>)> {
        let function = &self.module.functions[callee];
        let first = function
            .body
            .iter()
            .find(|statement| !matches!(statement, Statement::Emit(_)))?;
        match *first {
            Statement::Return { value: Some(value) } => Some((function, value)),
            _ => None,
        }
    }

    /// What the preprocessor can tell of `expression`, given what it can
    /// tell of the expressions before it in `function` (`partials`).
    fn partial(
        &self,
        function: &Function,
        expression: Handle<Expression>,
        partials: &[Partial],
        definitions: &mut Definitions,
    ) -> Partial {
        let arena = &function.expressions;
        let operand = |handle: Handle<Expression>| &partials[handle.index()];
        let partial = match arena[expression] {
            Expression::Literal(_) | Expression::Constant(_) | Expression::Override(_) => self
                .text(arena, expression)
                .filter(|_| self.integral(arena, expression))
                .map_or_else(Partial::unknown, Partial::known),
            Expression::Unary { op, expr } => {
                operand(expr).map(|value| unary(op, self.is_bool(arena, expr), value))
            }
            Expression::Binary { op, left, right } => {
                let bools = self.is_bool(arena, left) || self.is_bool(arena, right);
                operand(left).binary(op, bools, operand(right))
            }
            Expression::Select {
                condition,
                accept,
                reject,
            } => operand(condition).select(operand(accept), operand(reject)),
            Expression::As {
                expr,
                kind: ScalarKind::Sint | ScalarKind::Uint,
                ..
            } => operand(expr).clone(),
            Expression::As {
                expr,
                kind: ScalarKind::Bool,
                ..
            } => operand(expr).map(|value| format!("({value} != 0)")),
            Expression::Math {
                fun: fun @ (MathFunction::Max | MathFunction::Min),
                arg,
                arg1: Some(arg1),
                ..
            } => operand(arg).extreme(fun == MathFunction::Max, operand(arg1)),
            Expression::CallResult(callee) => match self.returned(callee) {
                Some((callee, value)) => self.partials(callee, definitions)[value.index()].clone(),
                None => Partial::unknown(),
            },
            _ => Partial::unknown(),
        };
        Partial {
            known: definitions.name(partial.known),
            value: definitions.name(partial.value),
        }
    }

    /// Whether the literal, constant or pipeline constant `expression` is a
    /// bool or an integer, which the preprocessor reads.
    fn integral(&self, arena: &Arena<Expression>, expression: Handle<Expression>) -> bool {
        let ty = match arena[expression] {
            Expression::Literal(literal) => return literal_text(literal).is_some(),
            Expression::Constant(constant) => self.module.constants[constant].ty,
            Expression::Override(constant) => self.module.overrides[constant].ty,
            _ => return false,
        };
        matches!(
            self.module.types[ty].inner,
            TypeInner::Scalar(Scalar {
                kind: ScalarKind::Bool | ScalarKind::Sint | ScalarKind::Uint,
                ..
            })
        )
    }

    /// What the preprocessor can tell of each expression of `function`.
    fn partials(&self, function: &Function, definitions: &mut Definitions) -> Vec<Partial> {
        let mut partials = Vec::with_capacity(function.expressions.len());
        for (handle, _) in function.expressions.iter() {
            let partial = self.partial(function, handle, &partials, definitions);
            partials.push(partial);
        }
        partials
    }
}

/// The longest preprocessor expression written out where it is used; a
/// longer one is a macro of its own ([`Definitions`]).
const MAX_INLINE: usize = 24;

/// Long preprocessor expressions, each a macro named by a hash of its text:
/// directives that combine them stay short, and the templates of one module
/// name an expression alike, so they share the code around it.
#[derive(Default)]
struct Definitions {
    names: HashMap<String, String>,
    /// Name and text, in the order made: a text names only earlier ones.
    texts: Vec<(String, String)>,
}

impl Definitions {
    /// `text`, or the macro that stands for it where it is long.
    fn name(&mut self, text: String) -> String {
        if text.len() <= MAX_INLINE {
            return text;
        }
        if let Some(name) = self.names.get(&text) {
            return name.clone();
        }
        let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
        let name = format!("CPD{:012x}", hash >> 16);
        self.names.insert(text.clone(), name.clone());
        self.texts.push((name.clone(), text));
        name
    }

    /// The definitions `directives` name, with those they name in turn.
    fn reached<'s>(&'s self, directives: &[String]) -> Vec<&'s (String, String)> {
        let mut wanted: Vec<&str> = Vec::new();
        let mut pending: Vec<&str> = directives.iter().map(String::as_str).collect();
        while let Some(text) = pending.pop() {
            for (at, _) in text.match_indices("CPD") {
                let name = &text[at..text.len().min(at + 15)];
                if !wanted.contains(&name)
                    && let Some((_, inner)) = self.texts.iter().find(|(known, _)| known == name)
                {
                    wanted.push(name);
                    pending.push(inner);
                }
            }
        }
        self.texts
            .iter()
            .filter(|(name, _)| wanted.contains(&name.as_str()))
            .collect()
    }
}

/// What the preprocessor can tell of a value: the condition under which the
/// pipeline constants decide it, and the value then, both integer
/// expressions over the macros. A bool is 1 or 0.
#[derive(Clone, Debug)]
struct Partial {
    known: String,
    value: String,
}

impl Partial {
    fn known(value: String) -> Self {
        Self {
            known: "1".to_string(),
            value,
        }
    }

    fn unknown() -> Self {
        Self {
            known: "0".to_string(),
            value: "0".to_string(),
        }
    }

    /// `self` with its value written by `write`, known where it is.
    fn map(&self, write: impl FnOnce(&str) -> String) -> Self {
        Self {
            known: self.known.clone(),
            value: write(&self.value),
        }
    }

    /// `self op other`, `bools` when they are bools. The preprocessor's
    /// division would fail on a divisor of zero the run never meets.
    fn binary(&self, op: BinaryOperator, bools: bool, other: &Self) -> Self {
        match op {
            BinaryOperator::And | BinaryOperator::LogicalAnd if bools => self.and(other),
            BinaryOperator::InclusiveOr | BinaryOperator::LogicalOr if bools => self.or(other),
            BinaryOperator::Divide | BinaryOperator::Modulo => Self::unknown(),
            _ => match symbol(op, bools) {
                Some(symbol) => Self {
                    known: and(&self.known, &other.known),
                    value: binary(symbol, &self.value, &other.value),
                },
                None => Self::unknown(),
            },
        }
    }

    /// `max(self, other)`, or `min` where not `max`.
    fn extreme(&self, max: bool, other: &Self) -> Self {
        let symbol = if max { ">" } else { "<" };
        let firsts = binary(symbol, &self.value, &other.value);
        Self {
            known: and(&self.known, &other.known),
            value: mix(&firsts, &self.value, &other.value),
        }
    }

    /// The value when it is known and true.
    fn holds(&self) -> String {
        and(&self.known, &self.value)
    }

    /// The value when it is known and false.
    fn fails(&self) -> String {
        and(&self.known, &not(&self.value))
    }

    /// Bool `self & other`: false as soon as either is known false.
    fn and(&self, other: &Self) -> Self {
        Self {
            known: or(
                &and(&self.known, &other.known),
                &or(&self.fails(), &other.fails()),
            ),
            value: and(&self.holds(), &other.holds()),
        }
    }

    /// Bool `self | other`: true as soon as either is known true.
    fn or(&self, other: &Self) -> Self {
        Self {
            known: or(
                &and(&self.known, &other.known),
                &or(&self.holds(), &other.holds()),
            ),
            value: or(&self.holds(), &other.holds()),
        }
    }

    /// `select(reject, accept, self)`: known when the condition is and the
    /// side it picks is.
    fn select(&self, accept: &Self, reject: &Self) -> Self {
        Self {
            known: or(
                &and(&self.holds(), &accept.known),
                &and(&self.fails(), &reject.known),
            ),
            value: mix(&self.value, &accept.value, &reject.value),
        }
    }
}

/// `first` where `condition`, a bool, holds, `second` otherwise, without
/// the conditional operator GLSL's preprocessor lacks.
fn mix(condition: &str, first: &str, second: &str) -> String {
    match (condition, first, second) {
        ("1", _, _) => first.to_string(),
        ("0", _, _) => second.to_string(),
        _ if first == second => first.to_string(),
        (_, _, "0") => binary("*", condition, first),
        (_, "0", _) => binary("*", &not(condition), second),
        _ => format!("({condition} * {first} + {} * {second})", not(condition)),
    }
}

/// `left symbol right`, worked out where both are integers.
fn binary(symbol: &str, left: &str, right: &str) -> String {
    if let (Ok(a), Ok(b)) = (left.parse::<i64>(), right.parse::<i64>())
        && let Some(value) = fold(symbol, a, b)
    {
        return value.to_string();
    }
    match (symbol, left, right) {
        ("*" | "&", "0", _) | ("*" | "&", _, "0") => "0".to_string(),
        ("*", "1", other) | ("*", other, "1") => other.to_string(),
        _ => format!("({left} {symbol} {right})"),
    }
}

/// `a symbol b` for two integers, where it has a value.
fn fold(symbol: &str, a: i64, b: i64) -> Option<i64> {
    let compared = match symbol {
        "==" => Some(a == b),
        "!=" => Some(a != b),
        "<" => Some(a < b),
        "<=" => Some(a <= b),
        ">" => Some(a > b),
        ">=" => Some(a >= b),
        _ => None,
    };
    if let Some(compared) = compared {
        return Some(i64::from(compared));
    }
    match symbol {
        "+" => a.checked_add(b),
        "-" => a.checked_sub(b),
        "*" => a.checked_mul(b),
        "&" => Some(a & b),
        "|" => Some(a | b),
        "^" => Some(a ^ b),
        "<<" => u32::try_from(b).ok().and_then(|b| a.checked_shl(b)),
        ">>" => u32::try_from(b).ok().and_then(|b| a.checked_shr(b)),
        _ => None,
    }
}

fn and(left: &str, right: &str) -> String {
    match (left, right) {
        ("0", _) | (_, "0") => "0".to_string(),
        ("1", other) | (other, "1") => other.to_string(),
        _ if left == right => left.to_string(),
        _ => format!("({left} && {right})"),
    }
}

fn or(left: &str, right: &str) -> String {
    match (left, right) {
        ("1", _) | (_, "1") => "1".to_string(),
        ("0", other) | (other, "0") => other.to_string(),
        _ if left == right => left.to_string(),
        _ => format!("({left} || {right})"),
    }
}

fn not(value: &str) -> String {
    match value {
        "1" => "0".to_string(),
        "0" => "1".to_string(),
        _ => match negated(value) {
            Some(inner) => inner.to_string(),
            None => format!("(!{value})"),
        },
    }
}

/// `inner` where `value` is `(!inner)` as a whole.
fn negated(value: &str) -> Option<&str> {
    let inner = value.strip_prefix("(!")?.strip_suffix(')')?;
    let mut depth = 0i32;
    for character in inner.chars() {
        depth += match character {
            '(' => 1,
            ')' => -1,
            _ => 0,
        };
        if depth < 0 {
            return None;
        }
    }
    (depth == 0).then_some(inner)
}

fn is_bool_type(module: &Module, ty: Handle<naga::Type>) -> bool {
    matches!(
        module.types[ty].inner,
        TypeInner::Scalar(Scalar {
            kind: ScalarKind::Bool,
            ..
        })
    )
}

fn literal_text(literal: Literal) -> Option<String> {
    Some(match literal {
        Literal::Bool(value) => u8::from(value).to_string(),
        Literal::U32(value) => value.to_string(),
        Literal::I32(value) => value.to_string(),
        Literal::AbstractInt(value) => value.to_string(),
        Literal::U64(value) => value.to_string(),
        Literal::I64(value) => value.to_string(),
        _ => return None,
    })
}

/// A constant literal as GLSL code.
fn literal_code(literal: Literal) -> Option<String> {
    Some(match literal {
        Literal::Bool(value) => value.to_string(),
        Literal::U32(value) => format!("{value}u"),
        Literal::I32(value) => format!("{value}"),
        Literal::F32(value) => format!("{value:?}"),
        _ => return None,
    })
}

/// The GLSL type a value of `kind` is cast to.
fn cast(kind: ScalarKind) -> Option<&'static str> {
    Some(match kind {
        ScalarKind::Bool => "bool",
        ScalarKind::Uint => "uint",
        ScalarKind::Sint => "int",
        ScalarKind::Float => "float",
        _ => return None,
    })
}

/// `op` applied to `operand`, in GLSL and its preprocessor alike: a bool's
/// complement is its negation.
fn unary(op: UnaryOperator, bools: bool, operand: &str) -> String {
    match op {
        UnaryOperator::LogicalNot => not(operand),
        UnaryOperator::BitwiseNot if bools => not(operand),
        UnaryOperator::BitwiseNot => format!("(~{operand})"),
        UnaryOperator::Negate => format!("(-{operand})"),
    }
}

/// How GLSL and its preprocessor write each binary operator.
const SYMBOLS: [(BinaryOperator, &str); 18] = [
    (BinaryOperator::Add, "+"),
    (BinaryOperator::Subtract, "-"),
    (BinaryOperator::Multiply, "*"),
    (BinaryOperator::Divide, "/"),
    (BinaryOperator::Modulo, "%"),
    (BinaryOperator::Equal, "=="),
    (BinaryOperator::NotEqual, "!="),
    (BinaryOperator::Less, "<"),
    (BinaryOperator::LessEqual, "<="),
    (BinaryOperator::Greater, ">"),
    (BinaryOperator::GreaterEqual, ">="),
    (BinaryOperator::And, "&"),
    (BinaryOperator::ExclusiveOr, "^"),
    (BinaryOperator::InclusiveOr, "|"),
    (BinaryOperator::LogicalAnd, "&&"),
    (BinaryOperator::LogicalOr, "||"),
    (BinaryOperator::ShiftLeft, "<<"),
    (BinaryOperator::ShiftRight, ">>"),
];

/// How they write the operators that take bools apart.
const BOOL_SYMBOLS: [(BinaryOperator, &str); 3] = [
    (BinaryOperator::And, "&&"),
    (BinaryOperator::InclusiveOr, "||"),
    (BinaryOperator::ExclusiveOr, "!="),
];

/// The symbol of `op` between operands that are bools when `bools`.
fn symbol(op: BinaryOperator, bools: bool) -> Option<&'static str> {
    let bool_symbols: &[(BinaryOperator, &str)] = if bools { &BOOL_SYMBOLS } else { &[] };
    bool_symbols
        .iter()
        .chain(&SYMBOLS)
        .find(|(candidate, _)| *candidate == op)
        .map(|(_, symbol)| *symbol)
}

/// The preprocessor conditions under which each side of an `if` can run.
struct Plan {
    accept: String,
    reject: String,
}

/// The preprocessor conditions under which `condition` can be true and can
/// be false, or none when no constants decide it.
fn plan(partials: &[Partial], condition: Handle<Expression>) -> Option<Plan> {
    let condition = &partials[condition.index()];
    (condition.known != "0").then(|| Plan {
        accept: not(&condition.fails()),
        reject: not(&condition.holds()),
    })
}

/// One function's pass over its body: each side of an `if` that the
/// constants can rule out goes under a directive, and each call is noted
/// with the conditions of the directives around it.
struct Transform<'a, 'd> {
    /// What the preprocessor can tell of each of the function's expressions.
    partials: Vec<Partial>,
    directives: &'a mut Directives<'d>,
    /// The conditions of the sides the pass is in.
    guards: Vec<String>,
    calls: Vec<(Handle<Function>, String)>,
}

impl<'a, 'd> Transform<'a, 'd> {
    fn new(
        analysis: &'a Analysis<'a>,
        function: &'a Function,
        directives: &'a mut Directives<'d>,
        definitions: &mut Definitions,
    ) -> Self {
        Self {
            partials: analysis.partials(function, definitions),
            directives,
            guards: Vec::new(),
            calls: Vec::new(),
        }
    }

    fn block(&mut self, block: &Block) -> Block {
        let mut out = Block::with_capacity(block.len());
        for (statement, span) in block.span_iter() {
            let span = *span;
            let statement = match *statement {
                Statement::If {
                    condition,
                    ref accept,
                    ref reject,
                } => {
                    let plan = plan(&self.partials, condition);
                    let (when_accept, when_reject) = match plan {
                        Some(plan) => (plan.accept, plan.reject),
                        None => ("1".to_string(), "1".to_string()),
                    };
                    Statement::If {
                        condition,
                        accept: self.side(accept, &when_accept, span),
                        reject: self.side(reject, &when_reject, span),
                    }
                }
                Statement::Call { function, .. } => {
                    let when = all_of(self.guards.iter().map(String::as_str));
                    self.calls.push((function, when));
                    statement.clone()
                }
                Statement::Block(ref inner) => Statement::Block(self.block(inner)),
                Statement::Loop {
                    ref body,
                    ref continuing,
                    break_if,
                } => Statement::Loop {
                    body: self.block(body),
                    continuing: self.block(continuing),
                    break_if,
                },
                Statement::Switch {
                    selector,
                    ref cases,
                } => Statement::Switch {
                    selector,
                    cases: cases
                        .iter()
                        .map(|case| naga::SwitchCase {
                            value: case.value,
                            body: self.block(&case.body),
                            fall_through: case.fall_through,
                        })
                        .collect(),
                },
                ref other => other.clone(),
            };
            out.push(statement, span);
        }
        out
    }

    /// A side of an `if` under the preprocessor condition `when`: kept
    /// whole for "1", emptied for "0".
    fn side(&mut self, block: &Block, when: &str, span: Span) -> Block {
        if when == "0" {
            return Block::new();
        }
        if when == "1" {
            return self.block(block);
        }
        self.guards.push(when.to_string());
        let inner = self.block(block);
        self.guards.pop();
        if inner.is_empty() {
            return inner;
        }
        let mut out = Block::with_capacity(inner.len() + 2);
        out.push(self.directives.directive(format!("#if {when}")), span);
        for (statement, span) in inner.span_into_iter() {
            out.push(statement, span);
        }
        out.push(self.directives.directive("#endif".to_string()), span);
        out
    }
}

/// The longest condition a function is put under; one longer keeps the
/// function in every template.
const MAX_REACH: usize = 2048;

/// "1" if any of `conditions` is, "0" if there are none, their disjunction
/// otherwise.
fn any_of<'s>(conditions: impl Iterator<Item = &'s str>) -> String {
    let mut terms: Vec<&str> = Vec::new();
    for condition in conditions {
        if condition == "1" {
            return "1".to_string();
        }
        if condition != "0" && !terms.contains(&condition) {
            terms.push(condition);
        }
    }
    let text = match terms.as_slice() {
        [] => "0".to_string(),
        [only] => (*only).to_string(),
        _ => format!("({})", terms.join(" || ")),
    };
    if text.len() > MAX_REACH {
        "1".to_string()
    } else {
        text
    }
}

/// "0" if any of `conditions` is, "1" if there are none, their conjunction
/// otherwise.
fn all_of<'s>(conditions: impl Iterator<Item = &'s str>) -> String {
    let mut terms: Vec<&str> = Vec::new();
    for condition in conditions {
        if condition == "0" {
            return "0".to_string();
        }
        if condition != "1" && !terms.contains(&condition) {
            terms.push(condition);
        }
    }
    match terms.as_slice() {
        [] => "1".to_string(),
        [only] => (*only).to_string(),
        _ => format!("({})", terms.join(" && ")),
    }
}

/// Under which constants the entry point `root` calls each function of
/// `module`, by handle index, from the calls each body makes in [`bodies`]
/// order; "0" for one it never calls. naga lets a function call only those
/// before it, so a function's callers are all known once the ones after it
/// are.
fn reachability(
    module: &Module,
    calls: &[Vec<(Handle<Function>, String)>],
    root: usize,
    definitions: &mut Definitions,
) -> Vec<String> {
    let count = module.functions.len();
    let mut terms: Vec<Vec<String>> = vec![Vec::new(); count];
    for (callee, when) in calls.get(count + root).into_iter().flatten() {
        terms[callee.index()].push(when.clone());
    }
    let mut reach = vec![String::new(); count];
    for index in (0..count).rev() {
        reach[index] = definitions.name(any_of(terms[index].iter().map(String::as_str)));
        for (callee, when) in &calls[index] {
            let term = all_of([reach[index].as_str(), when.as_str()].into_iter());
            terms[callee.index()].push(term);
        }
    }
    reach
}

/// A pipeline constant as its macro, with the default the shader gives it,
/// and how its uses in the shader's code are written.
struct OverrideMacro {
    name: String,
    kind: MacroKind,
    default: Option<String>,
    index: usize,
    /// Set by the pipeline: its default is a literal. A constant the shader
    /// derives from others is its definition, in `#if` and in code alike.
    settable: bool,
    code: String,
}

fn macros(module: &Module) -> Result<Vec<OverrideMacro>, String> {
    let analysis = Analysis { module };
    let mut found = Vec::new();
    for (handle, constant) in module.overrides.iter() {
        let name = constant
            .name
            .clone()
            .ok_or_else(|| "an unnamed pipeline constant".to_string())?;
        let kind = match module.types[constant.ty].inner {
            TypeInner::Scalar(Scalar {
                kind: ScalarKind::Bool,
                ..
            }) => MacroKind::Bool,
            TypeInner::Scalar(Scalar {
                kind: ScalarKind::Uint,
                ..
            }) => MacroKind::Uint,
            TypeInner::Scalar(Scalar {
                kind: ScalarKind::Sint,
                ..
            }) => MacroKind::Sint,
            TypeInner::Scalar(Scalar {
                kind: ScalarKind::Float,
                ..
            }) => MacroKind::Float,
            ref other => return Err(format!("{name}: a pipeline constant of {other:?}")),
        };
        let default = match constant.init {
            Some(init) => Some(match kind {
                MacroKind::Float => float_text(&module.global_expressions, init)
                    .ok_or_else(|| format!("{name}: a float default the generator cannot write"))?,
                _ => analysis
                    .text(&module.global_expressions, init)
                    .ok_or_else(|| format!("{name}: a default the preprocessor cannot read"))?,
            }),
            None => None,
        };
        let settable = constant
            .init
            .is_none_or(|init| matches!(module.global_expressions[init], Expression::Literal(_)));
        let code = analysis
            .override_code(handle)
            .ok_or_else(|| format!("{name}: a definition GLSL cannot be written from"))?;
        found.push(OverrideMacro {
            name,
            kind,
            default,
            index: handle.index(),
            settable,
            code,
        });
    }
    Ok(found)
}

fn float_text(arena: &Arena<Expression>, expression: Handle<Expression>) -> Option<String> {
    match arena[expression] {
        Expression::Literal(Literal::F32(value)) => Some(format!("{value:?}")),
        Expression::Literal(Literal::AbstractFloat(value)) => Some(format!("{value:?}")),
        _ => None,
    }
}

/// Each pipeline constant in `module`'s bodies as its numbered literal.
/// Errors on a literal already the size of a constant's.
fn mark_constants(module: &mut Module) -> Result<(), String> {
    let count = u32::try_from(module.overrides.len()).unwrap_or(u32::MAX);
    let sentinels = SENTINEL..SENTINEL.saturating_add(count);
    let functions = module.functions.iter_mut().map(|(_, function)| function);
    let entries = module
        .entry_points
        .iter_mut()
        .map(|entry| &mut entry.function);
    for function in functions.chain(entries) {
        for (_, expression) in function.expressions.iter_mut() {
            match *expression {
                Expression::Literal(Literal::U32(value)) if sentinels.contains(&value) => {
                    return Err("a literal the size of a constant's marker".to_string());
                }
                Expression::Override(constant) => {
                    let index = u32::try_from(constant.index()).unwrap_or(u32::MAX);
                    *expression = Expression::Literal(Literal::U32(SENTINEL.saturating_add(index)));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The written GLSL with its markers made directives, its numbered literals
/// made macros, and each macro's default after the `#version` line.
fn finish(
    written: &str,
    texts: &[String],
    macros: &[OverrideMacro],
    definitions: &Definitions,
) -> Result<String, String> {
    let (version, body) = written
        .split_once('\n')
        .ok_or_else(|| "no #version line".to_string())?;
    let mut code = Code {
        macros,
        text: String::with_capacity(written.len()),
        used: Vec::new(),
        block_start: 0,
        guarded_function: false,
    };
    let mut lines = body.lines();
    while let Some(line) = lines.next() {
        code.line(line, &mut lines, texts)?;
    }
    let mut out = String::with_capacity(code.text.len() + 2048);
    out.push_str(version);
    out.push('\n');
    for constant in macros {
        if let Some(default) = &constant.default {
            let _ = writeln!(
                out,
                "#ifndef CP_{0}\n#define CP_{0} {1}\n#endif",
                constant.name, default
            );
        }
    }
    for (name, text) in definitions.reached(&code.used) {
        let _ = writeln!(out, "#define {name} {text}");
    }
    out.push_str(&code.text);
    Ok(out)
}

/// The template's code as `finish` writes it, line by line.
struct Code<'m> {
    macros: &'m [OverrideMacro],
    text: String,
    /// The directives written, which name the definitions the header holds.
    used: Vec<String>,
    /// Where the latest top-level block, a function's signature among them,
    /// starts.
    block_start: usize,
    /// Whether the function being written closes a directive.
    guarded_function: bool,
}

impl Code<'_> {
    /// Writes the written `line`, taking from `rest` what a marker covers.
    fn line(
        &mut self,
        line: &str,
        rest: &mut std::str::Lines<'_>,
        texts: &[String],
    ) -> Result<(), String> {
        let trimmed = line.trim();
        // A marker's own definition: its signature to its closing brace.
        if trimmed.strip_prefix("void ").is_some_and(|signature| {
            signature.starts_with(MARKER.0)
                && signature.contains(MARKER.1)
                && signature.ends_with('{')
        }) {
            skip_function(rest);
            return Ok(());
        }
        if let Some(index) = trimmed
            .strip_prefix(MARKER.0)
            .and_then(|tail| tail.strip_suffix(&format!("{}();", MARKER.1)))
            .and_then(|digits| digits.parse::<usize>().ok())
        {
            let text = texts
                .get(index)
                .ok_or_else(|| format!("a marker {index} without a directive"))?;
            self.directive(text, rest);
            return Ok(());
        }
        if !line.starts_with(char::is_whitespace) && line.ends_with('{') {
            self.block_start = self.text.len();
        }
        let macros = self.macros;
        let in_code = |index: usize| -> Option<String> {
            let constant = macros.iter().find(|constant| constant.index == index)?;
            Some(constant.code.clone())
        };
        self.text.push_str(&with_macros(line, &in_code));
        self.text.push('\n');
        if self.guarded_function && line == "}" {
            self.text.push_str("#endif\n");
            self.guarded_function = false;
        }
        Ok(())
    }

    /// Writes the directive `text` where its marker stood.
    fn directive(&mut self, text: &str, rest: &mut std::str::Lines<'_>) {
        self.used.push(text.to_string());
        match text.strip_prefix(FUNCTION_GUARD) {
            // A function the entry point never calls goes whole.
            Some("0") => {
                self.text.truncate(self.block_start);
                skip_function(rest);
            }
            Some(when) => {
                self.text
                    .insert_str(self.block_start, &format!("#if {when}\n"));
                self.guarded_function = true;
            }
            None => {
                self.text.push_str(text);
                self.text.push('\n');
            }
        }
    }
}

/// Takes the lines of a function up to its closing brace.
fn skip_function(lines: &mut std::str::Lines<'_>) {
    for line in lines.by_ref() {
        if line == "}" {
            break;
        }
    }
}

/// `line` with each numbered literal written as its constant's macro.
fn with_macros(line: &str, in_code: &dyn Fn(usize) -> Option<String>) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index > start {
            let digits = &line[start..index];
            let replaced = (index < bytes.len() && bytes[index] == b'u')
                .then(|| digits.parse::<u64>().ok())
                .flatten()
                .and_then(|value| value.checked_sub(u64::from(SENTINEL)))
                .filter(|offset| *offset < 0x1_0000)
                .and_then(|offset| in_code(offset as usize));
            match replaced {
                Some(text) => {
                    out.push_str(&text);
                    index += 1;
                }
                None => out.push_str(digits),
            }
            continue;
        }
        let character = line[index..].chars().next().unwrap_or(' ');
        out.push(character);
        index += character.len_utf8();
    }
    out
}

/// The Rust source of a static for each of `modules`, named as given, that
/// holds the module's templates as `wgpu::hal::GlslTemplate`s, for a crate
/// that depends on `wgpu`. It imports `wgpu`'s `hal` and `naga` once and names
/// every type through them. Each piece of GLSL the templates share is written
/// once.
pub fn rust_source(modules: &[(&str, Vec<Template>)]) -> String {
    let mut pool: HashMap<&str, usize> = HashMap::new();
    let mut pieces_out = String::new();
    let mut statics = String::new();
    for (name, templates) in modules {
        let _ = write!(
            statics,
            "pub(crate) static {name}: &[hal::GlslTemplate] = &["
        );
        for template in templates {
            let generated = &template.generated;
            let _ = write!(
                statics,
                "hal::GlslTemplate {{ entry_point: {:?}, stage: naga::ShaderStage::{:?}, glsl: &[",
                template.entry_point, template.stage
            );
            for piece in pieces(&generated.glsl) {
                let next = pool.len();
                let index = *pool.entry(piece).or_insert_with(|| {
                    let _ = writeln!(pieces_out, "const P{next}: &str = {piece:?};");
                    next
                });
                let _ = write!(statics, "P{index},");
            }
            statics.push_str("], macros: &[");
            for (name, kind) in &generated.macros {
                let _ = write!(
                    statics,
                    "hal::GlslMacro {{ constant: {name:?}, name: \"CP_{name}\", kind: hal::GlslMacroKind::{kind:?} }},"
                );
            }
            statics.push_str("], blocks: &[");
            for block in &generated.blocks {
                let _ = write!(
                    statics,
                    "hal::GlslBlock {{ name: {:?}, storage: {}, binding: {} }},",
                    block.name,
                    block.storage,
                    resource_binding(block.binding)
                );
            }
            statics.push_str("], textures: &[");
            for texture in &generated.textures {
                let sampler = match texture.sampler {
                    Some(sampler) => format!("Some({})", resource_binding(sampler)),
                    None => "None".to_string(),
                };
                let _ = write!(
                    statics,
                    "hal::GlslTexture {{ name: {:?}, storage: {}, binding: {}, sampler: {sampler} }},",
                    texture.name,
                    texture.storage,
                    resource_binding(texture.binding)
                );
            }
            statics.push_str("], varyings: &[");
            for (name, location) in &generated.varyings {
                let _ = write!(statics, "({name:?}, {location}),");
            }
            let _ = write!(
                statics,
                "], clip_distance_count: {} }},",
                generated.clip_distance_count
            );
        }
        statics.push_str("];\n");
    }
    format!("use wgpu::{{hal, naga}};\n{pieces_out}{statics}")
}

/// `glsl` cut where templates are likely to share what follows: after the
/// `#version` line, each top-level declaration, each function and each
/// `#if` and `#endif`.
fn pieces(glsl: &str) -> impl Iterator<Item = &str> {
    let mut start = 0;
    let mut end = 0;
    let mut first = true;
    glsl.split_inclusive('\n').filter_map(move |line| {
        end += line.len();
        let text = line.trim_end();
        let top_level = !text.starts_with([' ', '\t', '#']);
        let cut = std::mem::take(&mut first)
            || text.starts_with('}')
            || text.starts_with("#endif")
            || text.starts_with("#if ")
            || (top_level && text.ends_with(';'))
            || end == glsl.len();
        let piece = cut.then(|| &glsl[start..end]);
        if cut {
            start = end;
        }
        piece
    })
}

fn resource_binding(binding: naga::ResourceBinding) -> String {
    format!(
        "naga::ResourceBinding {{ group: {}, binding: {} }}",
        binding.group, binding.binding
    )
}
