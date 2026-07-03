# sim-lib-binding

In one line: It keeps track of what every name in a program stands for, and exactly where that meaning holds.

## What it gives you

This library is the part of SIM that remembers which value each name points to as a program runs. It supports names that stay put inside the block that created them, names whose meaning can be swapped for the length of a call and then restored, and names defined together so they can refer to each other. When you read a name, you get the right value for the place you are standing in the code. When a temporary setting ends, the earlier one comes back on its own, so surprises stay rare and behavior stays predictable.

## Why you will be glad

- Local names stay local, so code in one place cannot quietly disturb another.
- Temporary settings unwind cleanly, even when a call ends early or fails.
- Groups of mutually referring definitions come up together without ordering headaches.

## Where it fits

Every language surface and library in SIM needs a shared, trustworthy answer to the question "what does this name mean here." This crate supplies that answer as one concrete organ built on the naming contracts the kernel defines. Other parts of the system lean on it rather than inventing their own scope rules, which keeps behavior consistent no matter which surface syntax you happen to be writing in.
