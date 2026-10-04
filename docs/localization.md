# Localization

Enable `cranpose`'s `localization` feature (or `cranpose-ui/localization` when
using the UI crate directly). Ordinary string literals keep their existing
meaning. Mark only text that should be translated:

```rust
Text(tr!("Save"), Modifier::empty(), TextStyle::default());
Text(tr!("Hello, {name}!", name = user.name.as_str()), modifier, style);
LiquidMenuItem::new(tr!("Open", context = "file action"));
Modifier::empty().content_description(tr!("Close dialog"));
```

`tr!` runs during composition. Its `LocalizedText` result shares immutable
annotated text with `Text` and converts to `String` for APIs that own strings.
Borrowed argument strings need live only through the call. Numeric arguments
stay numeric, so translators can choose plural forms. Do not translate pieces
of a sentence separately or preformat a numeric count into a string.

## Catalogs and language selection

Store Fluent files at `locales/<language>/<package-name>.ftl`. The namespace of
an inline `tr!` is the owning Cargo package's name, including hyphens. Embed the
catalogs once and provide a language at the app root:

```rust
let translations = translations!("locales", fallback = "en");
let language = rememberMutableStateOf(|| Locale::parse("en").expect("known locale"));
ProvideLocalization(&translations, language.value(), || App());
```

Changing `language` updates subscribed components, including accessibility
descriptions. Providers can be nested; separate windows and tests can use
different languages. `ProvideLocalization` supplies a layout-direction default.
Use `ProvideLayoutDirection` inside it for a region that must keep its direction.

`translations!` checks embedded Fluent syntax, references, duplicate definitions,
and argument contracts against source-language catalogs during compilation. It
permits missing translations because runtime fallback is supported. Existing
files use `include_str!`, so their edits trigger rebuilds. To also track added or
removed catalog files, add this line to your app's `build.rs`:

```rust
fn main() {
    println!("cargo::rerun-if-changed=locales");
}
```

Language identifiers preserve script and region (`sr-Latn`, `sr-Cyrl`, `fr-CA`).
To use several ordered language preferences supplied by your platform host:

```rust
let translator = translations.negotiate(&preferred_locales);
ProvideTranslator(translator, || App());
```

The host supplies the preferences; this API does not poll operating-system
settings. `Catalog::translator` and `Translator::format` also work without UI
composition, including background workers. Catalogs are immutable and shareable
across threads; construct a thread-owned translator on each worker so formatting
does not contend with the UI. Replace a catalog to install new resources; a provider observes
the replacement even when the selected language is unchanged.

Lookup tries negotiated languages, the configured fallback language, and then
the inline source. Each static source catalog retains one formatter per thread.
Each message carries its own source language, so an English
fallback uses English plural rules even when the requested language is Arabic.
Inline source defaults to English; specify `source_locale = "de"` for a German
source pattern. Source files used by generated accessors take their language
from the parent directory name.

## Message identity and extraction

Start with readable source text:

```rust
tr!("Save");
tr!("Open", context = "file action", comment = "Verb on a file menu");
tr!("Save changes", id = "editor-save");
```

Without an explicit ID, the ID is derived from the normalized source text and
context. Moving code does not change it; changing the wording does. Explicit
IDs survive wording changes. Different packages have separate namespaces.
`id`, `context`, `comment`, and `source_locale` are reserved macro options;
choose other names for interpolation arguments. Missing, extra, and duplicate
arguments are compile errors. For literal braces use Fluent string literals,
for example `{ "{" }`, rather than Rust format escapes.

From this repository, extract and validate with:

```sh
cargo run -p cranpose-localization --features tooling --bin cranpose-l10n -- \
  extract --source path/to/app/src --output path/to/app/locales/en/my-app.ftl
cargo run -p cranpose-localization --features tooling --bin cranpose-l10n -- \
  check --catalogs path/to/app/locales --fallback en
```

Published consumers can install `cranpose-l10n` with
`cargo install cranpose-localization --features tooling`. The same commands
then start with `cranpose-l10n`.

Extraction parses Rust tokens, including nested macros and code behind target
`cfg` attributes. It records source locations and translator guidance, combines
identical entries, and rejects conflicting explicit IDs. It updates the source
catalog while retaining handwritten Fluent entries and translator notes.
Previously extracted entries removed from code are removed from the source
catalog. Target-language files are never rewritten. Review source changes before
updating translations. Use `extract --check` in CI to reject a stale source
catalog and `check --allow-missing` when incomplete translations are intentional.

The extractor recognizes literal `tr!` invocations. A wrapper macro that generates
translation calls from Rust macro parameters needs its messages defined in a
Fluent file and generated accessors instead.

## Plurals, grammar, and generated accessors

For richer messages, write Fluent directly in a source file:

```ftl
files-remaining = { $count ->
    [one] One file remaining
   *[other] { $count } files remaining
    }
```

Generate Rust functions, with argument names and message names checked by Rust:

```rust
translation_messages!(pub mod messages, "locales/en/my-app.ftl");
Text(messages::files_remaining(count), modifier, style);
```

Hyphens become underscores. Colliding Rust function names are rejected.
Arguments accept Fluent-compatible text or numeric values; the accessor's
parameter list checks presence and uses alphabetical argument order. Pass numeric values to numeric
selectors. Terms and message references remain in the Fluent catalog, shared by
the generated accessors. Translators may introduce terms, reorder or omit
arguments, and add the plural categories their language needs; they cannot
require new application arguments.

Inline source also accepts Fluent select expressions when a small message is
more readable next to its component:

```rust
tr!("{ $count ->\n [one] One file\n *[other] { $count } files\n}", count = count);
```

Fluent's `NUMBER` builtin is supported. This API does not provide a date/time or
currency formatting service; prepare those values with an appropriate formatter.
It also does not turn translated text into markup or rich-text annotations.

## Framework strings and library overrides

Text-selection menus, caret-action menus, selection containers, default search
hints, and the tab-bar search accessory use the `cranpose-ui` namespace. English,
French, Serbian Latin, and Arabic catalogs are bundled only with localization
enabled. Application catalogs take precedence for matching messages; omitted
entries retain the library translation. For example `locales/fr/cranpose-ui.ftl`:

```ftl
copy = Copier le texte
```

Include the corresponding source-language namespace file to validate an
override's argument contract. A namespace without that file is accepted for
dependency overrides; its syntax and references are still checked.

Reusable libraries can expose a `Catalog`. Add it with
`application_catalog.with_fallback(&library_catalog)`. Both UI provider forms
add Cranpose's framework catalog automatically. Selection menus translate when
composed, and search specs use `placeholder: None` for a reactive default.
`Some(custom_text)` preserves an application's explicit hint. Tab labels accept
`SharedText`, `String`, and static literals; static literals do not allocate
when constructing a tab.

## Previewing and measuring

```rust
let locale = Locale::parse("en")?.with_preview(PreviewMode::Expanded);
let rtl = Locale::parse("en")?.with_preview(PreviewMode::Rtl);
```

Expanded preview adds accents and length. RTL preview applies bidi isolation and
an RTL layout default. It is a layout stress test, not an Arabic translation.
Supply fonts covering the selected scripts. Direction defaults use Cranpose's
existing layout-direction support; they do not establish complete RTL coverage
for every widget or change the shaping locale in every text style.

The runnable example exercises language switching, interpolation, plurals,
menus, tab labels, accessibility labels, and both preview modes:

```sh
cargo run -p cranpose --example localization --features desktop,renderer-wgpu,localization
just test-localization
just bench-localization
```

Catalogs and inline sources are parsed once on first use. Create application
catalogs before starting the event loop; steady-state frames do not parse them.
Each composition position retains only its last result and
argument snapshot. Unchanged calls reuse annotated text; changing arguments
reuses the snapshot's storage. The cache reads the reactive translator before
looking for a hit. It is released with its composition position. APIs requiring
an owned `String` still copy shared content at that ownership boundary.

`localization` is off by default. Apps that do not enable it have no Fluent
runtime dependency. The benchmark compares repeated plain and localized text,
changing arguments, and language changes. Desktop benchmark results are
diagnostic only; performance acceptance requires the repository's comparison
against the last release on the slowest shipped physical device.
