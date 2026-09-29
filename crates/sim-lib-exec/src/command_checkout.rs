// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `CommandSpec` builder methods binding an interpreter script to a manifest
//! field, and adding an exact input checkout.

use super::*;

impl CommandSpec {
    /// Binds the interpreter script to one owner manifest field and
    /// re-derives the command identity.
    ///
    /// # Errors
    /// Refuses a repeated selection, a non-interpreter command, a manifest
    /// resource that is not declared read-only, a non-canonical path, and a
    /// table, name or field that is empty, oversized or not a plain key.
    pub fn with_manifest_selection(mut self, selection: ManifestSelection) -> Result<Self> {
        if self.manifest.is_some() {
            return Err(Error::Eval("command manifest is already selected".into()));
        }
        if !matches!(self.invocation, CommandInvocation::Interpreter { .. }) {
            return Err(Error::Eval(
                "only an interpreter command takes its script from a manifest".into(),
            ));
        }
        let read_only = self.resources.iter().any(|resource| {
            resource.source == selection.resource && resource.access == ResourceAccess::ReadOnly
        });
        let plain = |value: &str| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        };
        if !read_only
            || !canonical_relative_path(&selection.path)
            || !plain(&selection.table)
            || !plain(&selection.name)
            || !plain(&selection.field)
        {
            return Err(Error::Eval("invalid command manifest selection".into()));
        }
        self.manifest = Some(selection);
        self.id = CommandId(
            self.canonical_without_id()
                .content_id()
                .map_err(|_| Error::Eval("command specification is not canonical".into()))?,
        );
        Ok(self)
    }
    /// Adds an exact input checkout and re-derives the command identity.
    ///
    /// # Errors
    /// Refuses an empty or oversized checkout, a repeated selection, a source
    /// that is not a declared read-only resource, a working root that is not a
    /// declared writable resource, a working root that is not disposable
    /// cleanup scratch, a duplicate target, and any path that is not a
    /// canonical relative path.
    pub fn with_checkout(mut self, files: Vec<CheckoutFile>) -> Result<Self> {
        if !self.checkout.is_empty() {
            return Err(Error::Eval("command checkout is already selected".into()));
        }
        if files.is_empty() || files.len() > MAX_CHECKOUT_FILES {
            return Err(Error::Eval("command checkout size is not finite".into()));
        }
        let access = |name: &str| {
            self.resources
                .iter()
                .find(|resource| resource.source == name)
                .map(|resource| resource.access)
        };
        if access(self.root.as_str()) != Some(ResourceAccess::Writable) {
            return Err(Error::Eval(
                "command checkout requires a writable working root".into(),
            ));
        }
        if !self
            .cleanup
            .scratch_resources()
            .contains(self.root.as_str())
        {
            return Err(Error::Eval(
                "command checkout requires its working root to be disposable scratch".into(),
            ));
        }
        let mut targets = BTreeSet::new();
        for file in &files {
            // `.sim-` names are reserved for the owner's own scratch records,
            // such as its acceptance canary and atomic-replace temporaries.
            let reserved = file.target.split('/').any(|part| part.starts_with(".sim-"));
            if access(&file.resource) != Some(ResourceAccess::ReadOnly)
                || !canonical_relative_path(&file.path)
                || !canonical_relative_path(&file.target)
                || reserved
                || !targets.insert(file.target.as_str())
            {
                return Err(Error::Eval(
                    "invalid or duplicate command checkout file".into(),
                ));
            }
        }
        // A target may not be a directory of another target.
        if targets.iter().any(|left| {
            targets.iter().any(|right| {
                right
                    .strip_prefix(left)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            })
        }) {
            return Err(Error::Eval("command checkout targets are nested".into()));
        }
        // An output of the working root must be a target itself or unrelated to
        // every target: a directory above or a path inside a target could never
        // hold the absent or copied pre-image the owner must record.
        let nested = |left: &str, right: &str| {
            right
                .strip_prefix(left)
                .is_some_and(|suffix| suffix.starts_with('/'))
        };
        if self.outputs.outputs().iter().any(|output| {
            output.resource == self.root.as_str()
                && targets.iter().any(|target| {
                    nested(&output.relative_path, target) || nested(target, &output.relative_path)
                })
        }) {
            return Err(Error::Eval(
                "command output lies above or inside a checkout target".into(),
            ));
        }
        let mut files = files;
        files.sort();
        self.checkout = files;
        self.id = CommandId(
            self.canonical_without_id()
                .content_id()
                .map_err(|_| Error::Eval("command specification is not canonical".into()))?,
        );
        Ok(self)
    }
}
