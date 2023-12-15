# jsondiffpatch

Diff & patch JSON values, in Rust.

This crate is a port of the JavaScript library [jsondiffpatch](https://github.com/benjamine/jsondiffpatch) 0.6.0 that
produces exactly what the original produces: the same deltas, the same patched values, the same formatter output and
the same error messages. It has no dependencies beyond the Rust standard library.

- smart array diffing using [LCS](http://en.wikipedia.org/wiki/Longest_common_subsequence_problem), with move
  detection; to match objects inside an array, give an `objectHash` property name (otherwise objects are matched by
  position). See the [array diff documentation](docs/arrays.md)
- long text diffs at character level, with a port of
  [google-diff-match-patch](https://github.com/google/diff-match-patch) (`src/dmp`, Apache-2.0, see
  [its licence](src/dmp/Apache-2.0.txt))
- reverse a delta, unpatch (revert a value to its original state using a delta)
- the simple, pure JSON [delta format](docs/deltas.md)
- formatters: JSON Patch (RFC 6902), annotated JSON (HTML), HTML and console (without colours). See the
  [formatters documentation](docs/formatters.md)

## Library

```rust
use jsondiffpatch::js::json;
use jsondiffpatch::{DiffPatcher, Options};

let left = json::parse_str(r#"{"name":"South America","population":385742554}"#)?;
let right = json::parse_str(r#"{"name":"South America","population":385744896}"#)?;
let diffpatcher = DiffPatcher::new(Options::default());
let delta = diffpatcher.diff(&left, &right)?;
// {"population":[385742554,385744896]}
let patched = diffpatcher.patch(left, &delta)?;
let original = diffpatcher.unpatch(patched, &delta)?;
let reversed = diffpatcher.reverse(&delta)?;
let html = jsondiffpatch::formatters::html::format(&delta, &original)?;
```

Values are `jsondiffpatch::Value`s, which follow JavaScript semantics (UTF-16 strings, IEEE numbers, property order,
property access on any value), because that is what deltas are defined in terms of.

## Command

`jsondiffpatch` reads one JSON request per line on standard input and writes one JSON response per line:

```sh
$ echo '{"op":"diff","left":{"a":1,"b":[1,2,3]},"right":{"a":2,"b":[3,1,2]}}' | jsondiffpatch
{"delta":{"a":[1,2],"b":{"_t":"a","_2":["",0,3]}}}
```

Requests:

- `{"op":"diff","left":L,"right":R}`, `{"op":"patch","left":L,"delta":D}`, `{"op":"unpatch","right":R,"delta":D}`,
  `{"op":"reverse","delta":D}` and `{"op":"format","delta":D,"left":L}`
- `"options"`: `objectHash` (a property name), `matchByPosition`, `arrays` (`detectMove`, `includeValueOnMove`),
  `textDiff` (`minLength`), `propertyFilter` (property names to leave out) and `cloneDiffValues`
- `"format"`: `delta` (the default), `jsonpatch`, `annotated`, `console` or `html`
- `"reviveDates"`: turn ISO date strings in `left`, `right` and `delta` into dates first

Responses are `{"delta":...}`, `{"result":...}`, `{"jsonpatch":[...]}`, `{"text":"..."}` or
`{"error":{"name":"...","message":"..."}}`. The exit status is 0 when every request succeeded, 1 when one failed and
2 for command-line arguments (there are none).

## Building

```sh
cargo build --release
cargo test --release
```

## License

MIT, see [MIT-LICENSE.txt](MIT-LICENSE.txt). The diff-match-patch port in `src/dmp` is Apache-2.0, see
[src/dmp/Apache-2.0.txt](src/dmp/Apache-2.0.txt).
