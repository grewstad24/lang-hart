# Hart

## What is this project?

Hart is a compiled, general-purpose programming language focused on combining portable execution, dynamic ergonomics and simple syntax found in interpreted languages with the performance and safety found in systems languages.

## Why does it exist?

Hart was created to remove the choice between performance and ergonomics when writing a program. It achieves many features of interpreted languages like "Build Once, Run Anywhere" and dynamic-ish typing while keeping a solid performance and compile-time safety profile.

## What can it do?

- Hart compiles code to the APE format, leveraging the Cosmopolitan libc toolchain to allow a program to be compiled once and be run on any supported operating system and CPU architecture.

- Hart uses lazy function specialisation to provide ergonomics similar to dynamic typing.
  ```
  import { .print }

  def add(a, b) { a + b }

  func main() -> :none {
        println(add(1, 2))
        println(add(1 as u16, 2 as u16))
  }
  ```
  The add function automatically specialises to two concrete implementations for each set of types. Output:
  ```
  3
  3
  ```

## How do I install and use it?

Hart is not currently in a usable or viable state. It is missing a large amount of basic functionality.

## How can I contribute?

Contributions are not currently being accepted. Hart aims to be open for community contributions after it develops further.
