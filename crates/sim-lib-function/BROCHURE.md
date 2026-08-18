# Language-neutral function plans

In one line: It separates reusable function shape and capture mechanics from the guest-language policy that executes a function body.

## What it gives you

This library describes functions as checked plans and managed instances. A plan records positional, named, optional, rest, and keyword parameters together with capture descriptors, call mode, browse projection, and stable validation errors. Binding keeps argument origins visible and produces one ordered result that dispatch can consume without guessing how inputs arrived. Instances validate captured bindings before retaining them and delegate body behavior through an explicit policy type, so a Java lambda, Python closure, Lisp function, or native adapter can share mechanics without sharing semantics. The resulting callable body enters SIM through the ordinary method dispatch contract.

## Why you will be glad

- Parameter binding and capture validation have one inspectable owner.
- Guest runtimes can supply execution policy without copying neutral call machinery.
- Managed instances retain explicit capture evidence instead of hidden host closures.
- Typed plan and instance errors make invalid definitions fail before execution.

## Where it fits

This is the function organ beside Shape-based binding, dispatch, and managed mutation. It does not parse a guest language or choose evaluation rules; source profiles and adapters build plans, provide body policy, and expose the resulting instances through their own loadable libraries.
