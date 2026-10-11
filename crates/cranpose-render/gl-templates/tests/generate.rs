//! Templates through the generator's entry point, expanded by the GL
//! backend for a pipeline's constants.

use cranpose_gl_templates::{MacroKind, Template, module_templates};
use cranpose_ui_graphics::framework_shaders as wgsl;

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// `template` as the build script writes it for the GL backend.
fn hal_template(template: &Template) -> wgpu_hal::GlslTemplate {
    let macros: Vec<wgpu_hal::GlslMacro> = template
        .generated
        .macros
        .iter()
        .map(|(name, kind)| wgpu_hal::GlslMacro {
            constant: leak(name.clone()),
            name: leak(format!("CP_{name}")),
            kind: match kind {
                MacroKind::Bool => wgpu_hal::GlslMacroKind::Bool,
                MacroKind::Sint => wgpu_hal::GlslMacroKind::Sint,
                MacroKind::Uint => wgpu_hal::GlslMacroKind::Uint,
                MacroKind::Float => wgpu_hal::GlslMacroKind::Float,
            },
        })
        .collect();
    wgpu_hal::GlslTemplate {
        entry_point: leak(template.entry_point.clone()),
        stage: template.stage,
        glsl: Box::leak(vec![leak(template.generated.glsl.clone())].into_boxed_slice()),
        macros: Box::leak(macros.into_boxed_slice()),
        blocks: &[],
        textures: &[],
        varyings: &[],
        clip_distance_count: 0,
    }
}

/// The template's GLSL for a pipeline whose `constants` are given by name.
fn expand(template: &Template, constants: &[(&str, f64)]) -> Result<String, String> {
    let constants: naga::back::PipelineConstants = constants
        .iter()
        .map(|(name, value)| (name.to_string(), *value))
        .collect();
    hal_template(template).expand(&constants)
}

/// The names of the functions `glsl` defines.
fn functions(glsl: &str) -> Vec<&str> {
    glsl.lines()
        .filter(|line| !line.starts_with([' ', '#']) && line.ends_with('{'))
        .filter_map(|line| line.split_once('(').map(|(head, _)| head))
        .filter_map(|head| head.split_whitespace().last())
        .collect()
}

/// Constants to build each template with: none, every one at its lowest and
/// at its highest, and each alone at its highest.
fn assignments(template: &Template) -> Vec<Vec<(&str, f64)>> {
    let value = |kind: MacroKind, high: bool| -> f64 {
        match (kind, high) {
            (MacroKind::Bool, high) => f64::from(u8::from(high)),
            (MacroKind::Uint, false) => 0.0,
            (MacroKind::Uint, true) => 65535.0,
            (MacroKind::Sint, false) => -1.0,
            (MacroKind::Sint, true) => 7.0,
            (MacroKind::Float, false) => 0.0,
            (MacroKind::Float, true) => 4.5,
        }
    };
    let macros = &template.generated.macros;
    let every = |high: bool| -> Vec<(&str, f64)> {
        macros
            .iter()
            .map(|(name, kind)| (name.as_str(), value(*kind, high)))
            .collect()
    };
    let mut out = vec![Vec::new(), every(false), every(true)];
    for (name, kind) in macros {
        out.push(vec![(name.as_str(), value(*kind, true))]);
    }
    out
}

#[test]
fn every_built_in_template_preprocesses_under_any_constants() {
    for (module, source) in wgsl::MODULES {
        let templates =
            module_templates(source).unwrap_or_else(|error| panic!("{module}: {error}"));
        assert!(!templates.is_empty(), "{module} has entry points");
        for template in &templates {
            let name = format!("{module} {}", template.entry_point);
            assert!(
                template.generated.glsl.starts_with("#version 300 es\n"),
                "{name}"
            );
            for constants in assignments(template) {
                let glsl = expand(template, &constants)
                    .unwrap_or_else(|error| panic!("{name} with {constants:?}: {error}"));
                assert!(glsl.contains("void main()"), "{name} keeps its entry point");
                let (directives, code): (Vec<&str>, Vec<&str>) =
                    glsl.lines().partition(|line| line.starts_with('#'));
                let defined: Vec<&str> = directives
                    .iter()
                    .filter_map(|line| line.strip_prefix("#define "))
                    .filter_map(|rest| rest.split_whitespace().next())
                    .collect();
                assert_eq!(
                    directives.len(),
                    defined.len() + 1,
                    "{name} keeps only its #version line and definitions"
                );
                for word in code
                    .iter()
                    .flat_map(|line| line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')))
                    .filter(|word| word.starts_with("CP"))
                {
                    assert!(
                        defined.contains(&word),
                        "{name} reads {word}, which it does not define"
                    );
                }
                let kept = functions(&glsl);
                for function in functions(&template.generated.glsl) {
                    assert!(
                        kept.contains(&function) || !glsl.contains(&format!("{function}(")),
                        "{name} with {constants:?} calls {function}, which it dropped"
                    );
                }
            }
        }
    }
}

const BRANCHES: &str = "
override CUT: bool = false;
override KINDS: u32 = 3u;
override LINES: bool = (KINDS & 2u) != 0u;
override SCALE: f32 = 1.0;
override FIXED_KIND: i32 = -1;

@group(0) @binding(0) var<uniform> level: vec4<f32>;

fn draws_arcs() -> bool {
    return (KINDS & 4u) != 0u;
}

fn arc_blue() -> f32 {
    return 0.4375;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    var color = vec4<f32>(0.0);
    if (CUT & (position.x > level.x)) {
        color.r = 0.125;
    } else {
        color.g = 0.25;
    }
    if (LINES) {
        color.b = 0.375;
    }
    if (draws_arcs()) {
        color.b = arc_blue();
    }
    let kind = select(u32(position.z), u32(max(FIXED_KIND, 0)), FIXED_KIND >= 0);
    if (kind == 1u) {
        color.r = 0.75;
    }
    if (position.y > level.y) {
        color.a = 0.5;
    } else {
        color.a = 0.625;
    }
    return color * SCALE;
}
";

fn branches(constants: &[(&str, f64)]) -> String {
    let templates = module_templates(BRANCHES).expect("generates");
    expand(&templates[0], constants).expect("expands")
}

/// A condition joins a constant to a run-time value with `&`: naga writes
/// WGSL's `&&` with a run-time side as a branch of its own.
#[test]
fn a_side_stays_where_the_constants_leave_it_to_the_run() {
    let cut = branches(&[("CUT", 1.0)]);
    assert!(cut.contains("0.125") && cut.contains("0.25"));
    let kept = branches(&[("CUT", 0.0)]);
    assert!(!kept.contains("0.125") && kept.contains("0.25"));
}

/// A function only a ruled-out side calls goes with it.
#[test]
fn a_side_goes_where_a_derived_constant_or_a_helper_rules_it_out() {
    let none = branches(&[("KINDS", 1.0)]);
    assert!(!none.contains("0.375") && !none.contains("0.4375"));
    let lines = branches(&[("KINDS", 2.0)]);
    assert!(lines.contains("0.375") && !lines.contains("0.4375"));
    let arcs = branches(&[("KINDS", 4.0)]);
    assert!(!arcs.contains("0.375") && arcs.contains("0.4375"));
}

#[test]
fn a_branch_no_constant_decides_keeps_both_sides() {
    for kinds in [0.0, 7.0] {
        let glsl = branches(&[("KINDS", kinds), ("CUT", 1.0)]);
        assert!(glsl.contains("0.5") && glsl.contains("0.625"));
    }
}

#[test]
fn only_constants_a_pipeline_sets_are_macros_it_defines() {
    let templates = module_templates(BRANCHES).expect("generates");
    let mut names: Vec<&str> = templates[0]
        .generated
        .macros
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["CUT", "FIXED_KIND", "KINDS", "SCALE"]);
    let scaled = branches(&[("SCALE", 2.5)]);
    assert!(
        scaled.contains("#define CP_SCALE 2.5") && scaled.contains("float(CP_SCALE)"),
        "{scaled}"
    );
}

/// A string literal as `{:?}` writes it, read back.
fn unescape(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('u') => {
                let code: String = chars.by_ref().skip(1).take_while(|c| *c != '}').collect();
                let value = u32::from_str_radix(&code, 16).expect("a code point");
                out.push(char::from_u32(value).expect("a char"));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[test]
fn the_written_source_puts_each_template_together_from_its_pieces() {
    let modules: Vec<(&str, Vec<Template>)> = wgsl::MODULES
        .iter()
        .take(2)
        .map(|(name, source)| (*name, module_templates(source).expect("generates")))
        .collect();
    let source = cranpose_gl_templates::rust_source(&modules);
    let mut pieces = std::collections::HashMap::new();
    for line in source.lines().filter(|line| line.starts_with("const P")) {
        let (name, literal) = line
            .strip_prefix("const ")
            .and_then(|rest| rest.split_once(": &str = \""))
            .expect("a piece");
        let literal = literal.strip_suffix("\";").expect("a closed literal");
        pieces.insert(name.to_string(), unescape(literal));
    }
    let lists: Vec<&str> = source
        .split("glsl: &[")
        .skip(1)
        .map(|rest| rest.split_once(']').expect("a closed list").0)
        .collect();
    let templates: Vec<&Template> = modules
        .iter()
        .flat_map(|(_, templates)| templates)
        .collect();
    assert_eq!(lists.len(), templates.len());
    for (list, template) in lists.iter().zip(templates) {
        let glsl: String = list
            .split(',')
            .filter(|name| !name.is_empty())
            .map(|name| pieces[name].as_str())
            .collect();
        assert_eq!(glsl, template.generated.glsl, "{}", template.entry_point);
    }
}

/// An integer constant picked by `select` decides a comparison, as a brush
/// kind a pipeline fixes does.
#[test]
fn a_side_goes_where_a_selected_integer_constant_rules_it_out() {
    assert!(!branches(&[("FIXED_KIND", 0.0)]).contains("0.75"));
    assert!(branches(&[("FIXED_KIND", 1.0)]).contains("0.75"));
    assert!(branches(&[("FIXED_KIND", -1.0)]).contains("0.75"));
}
