# sim-lib-lang-prolog

In one line: It lets you write for SIM in Prolog style, stating facts and rules and letting the system find the answers.

## What it gives you

This library gives SIM a Prolog face. In the Prolog way of working you do not spell out steps; you state what is true and what follows from what, then ask a question and let the system search for every set of values that fits. This profile brings that declarative style to SIM and connects it to the shared reasoning engine, so your rules and queries run alongside everything else. You get the familiar Prolog feel -- facts, rules, and questions with blanks to fill -- resting on a common foundation instead of a separate tool.

## Why you will be glad

- You describe what holds and let the search produce the answers for you.
- The familiar Prolog style of facts, rules, and queries carries over directly.
- Your reasoning runs on the shared engine, beside the rest of the runtime.

## Where it fits

The kernel defines the expression and library contracts. This crate installs a Prolog-flavored logic policy and registers the Prolog callable surface over the shared logic query engine that the logic organ provides. It is the language face that turns that reasoning core into the notation Prolog people expect, one of several surfaces the runtime can present on common underlying ground.
