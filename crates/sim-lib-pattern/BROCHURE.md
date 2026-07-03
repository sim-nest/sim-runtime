# sim-lib-pattern

In one line: It takes data apart by its shape and handles each case, warning you when a case is missed.

## What it gives you

This library lets you describe the shapes your data can take and then respond to each one directly. Instead of poking at a value piece by piece, you write out the forms it might have and let the match pull the parts you care about into named pieces for you. You can define your own kinds of structured values with clearly listed variants, and when you handle them the checker tells you if you have left a possibility unaddressed. That gap-catching turns a common class of quiet bugs into a plain, up-front message.

## Why you will be glad

- Pulling fields out of structured data reads as one clear statement, not a pile of checks.
- Your own data kinds can list their variants, so intent is written down plainly.
- Forgetting a case is caught for you instead of surfacing later as a bug.

## Where it fits

The kernel provides the Shape protocol that describes how a value can be matched and its parts bound. This crate is the concrete pattern organ built on Shape: algebraic data types, destructuring, match arms, and exhaustiveness checking. Language surfaces that offer pattern matching express it through here, so taking data apart works the same way and stays checked no matter the syntax above.
