# IntelliJ Lira Plugin

Syntax highlighting for Lira programming language in JetBrains IDEs.

## Installation

### From JetBrains Marketplace (Coming Soon)

1. Open your JetBrains IDE (IntelliJ IDEA, WebStorm, etc.)
2. Go to Settings/Preferences > Plugins
3. Search for "Lira"
4. Click Install

### Manual Installation

1. Download the latest `.zip` from releases
2. Go to Settings/Preferences > Plugins > Gear icon > Install Plugin from Disk
3. Select the downloaded `.zip` file

### Build from Source

```bash
cd editors/intellij-lira
./gradlew buildPlugin
# Plugin will be in build/distributions/
```

## Features

- Syntax highlighting for `.li` and `.lira` files
- Comment toggling (Ctrl+/)
- Bracket matching

## Highlighted Elements

- **Keywords**: `fn`, `let`, `var`, `const`, `struct`, `class`, `enum`, `trait`, `impl`, etc.
- **Control Flow**: `if`, `else`, `match`, `while`, `for`, `loop`, `break`, `continue`, `return`
- **Concurrency**: `spawn`, `select`, `async`
- **Types**: `int`, `float`, `bool`, `string`, `List`, `Map`, `Option`, `Result`, etc.
- **Literals**: Numbers, strings, characters, booleans
- **Comments**: Line (`//`) and block (`/* */`) comments
- **String Interpolation**: `${expression}`

## Development

### Prerequisites

- JDK 17+
- Gradle 8+

### Build

```bash
./gradlew build
```

### Run in Development IDE

```bash
./gradlew runIde
```

### Package

```bash
./gradlew buildPlugin
```

## License

MIT
