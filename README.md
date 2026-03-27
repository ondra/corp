# `corp`

An efficient corpus access package written in Rust compatible with the binary format used by [Manatee](https://no.sketchengine.eu) / [Sketch Engine](https://www.sketchengine.eu/).

## Installation

### Prebuilt binaries

Prebuilt statically linked binaries for x86_64 Linux are available in the
`bin/` directory. They have no external dependencies.

### Install from source

```
cargo install --git https://github.com/ondra/corp
```

### Build from source

```
rustup default stable
cargo build --release
```

The binaries will be in `target/release/`.

### Building static binaries

To build fully static binaries for distribution (requires the musl
toolchain):

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

The binaries will be in `target/x86_64-unknown-linux-musl/release/`.

## Tools

Note that `<config>`, the path to the corpus configuration file is relative with respect to the `MANATEE_REGISTRY` environment variable by default. To override this behavior, use absolute path to the configuration file or prefix it with `./` to use path relative to the current working directory.

### encodevert

Compile vertical text into binary corpus format.

```
encodevert [-c] <config> [input]
```

If input is omitted, the `VERTICAL` directive from the configuration is used if it exists, otherwise `stdin` is read.

### mkrev

Create reverse index for an attribute.

```
mkrev <config> <attribute>
```

### mkdynattr

Create a dynamic attribute derived from an existing one (e.g., a lowercased
version of the word attribute).

```
mkdynattr <config> <attribute>
```

### mkstats

Generate frequency statistics for an attribute.

```
mkstats <config> <attribute> frq
```

### mktokencov

Calculate token coverage for structure attributes. Used for normalizing
frequency counts across sub-parts of the corpus (e.g., documents, time
periods).

```
mktokencov <config> [structure [attribute]]
```

### lswl

List the most frequent items in an attribute, sorted by frequency.

```
lswl [OPTIONS] <config> <attribute>
```

Options:
- `-l N` — number of items to return (default 100)
- `-i N` — minimum frequency (default 5)
- `-a N` — maximum frequency, 0 disables (default 0)
- `-s TYPE` — sort frequency type (default frq)

### conc

A rudimentary Concordancer. Only single token searches are supported.

```
conc [OPTIONS] <config> <word>
```

Options:
- `-a ATTR` — attribute to display (default: word)
- `-q ATTR` — attribute to query (default: same as -a)
- `-w N` — context window size (default 15)
- `-l N` — limit number of results
- `-n N` — random sample size
- `-t` — tab-separated output

### corpinfo

Display corpus metadata.

```
corpinfo [OPTIONS] <config>
```

Options:
- `-p` — print corpus path
- `-s` — print corpus size (number of tokens)
- `-v` — print version

### mksubc

Create subcorpora from structure/query definitions.

```
mksubc <config> <output> <expression>
```

### genvert

Generate synthetic vertical text data with Zipfian frequency distribution for testing purposes.

## Example

A complete working example is provided in `examples/`. See
[examples/EXAMPLE.md](examples/EXAMPLE.md) for a step-by-step walkthrough.

Quick start:

```bash
cd examples
encodevert -c ./testcorp.conf
mkrev ./testcorp.conf word
mkrev ./testcorp.conf doc.id
mkrev ./testcorp.conf doc.date
mkdynattr ./testcorp.conf lc
mkstats ./testcorp.conf word frq
mktokencov ./testcorp.conf
lswl -i 1 ./testcorp.conf word
conc ./testcorp.conf the
```

## Configuration

The corpus configuration file describes the structure of the corpus. Example:

```
PATH "/output/data/directory"
VERTICAL "./input.vert"
DEFAULTATTR word
ATTRIBUTE word
ATTRIBUTE lemma
ATTRIBUTE lc {
    DYNLIB internal
    DYNTYPE freq
    DYNAMIC utf8lowercase
    FROMATTR word
}
STRUCTURE s
STRUCTURE doc {
    ATTRIBUTE date
    ATTRIBUTE id
}
```

For the full configuration format, see the
[Sketch Engine documentation](https://www.sketchengine.eu/documentation/corpus-configuration-file-all-features/).

Note: when specifying the corpus to the tools, use a path starting with `.`
or `/` to avoid lookup via the `MANATEE_REGISTRY` environment variable.

## License

GPL-3.0. See [LICENSE](LICENSE).
