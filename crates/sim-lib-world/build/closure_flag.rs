// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Closure word for one lock package.

/// A path package in the dependency closure stays inside it.
///
/// A registry or git package stays inside only when this compilation
/// reached its bytes. An entry this compilation did not download is
/// outside, and the seal keeps that entry present-unverified.
pub(crate) fn closure_word(in_closure: bool, origin: &str, observed: &str) -> &'static str {
    if in_closure && (origin == "path" || !observed.is_empty()) {
        "closure"
    } else {
        "outside"
    }
}

#[cfg(test)]
mod tests {
    use super::closure_word;

    #[test]
    fn a_reached_registry_package_stays_in_the_closure() {
        assert_eq!(closure_word(true, "registry", "abcdef"), "closure");
    }

    #[test]
    fn an_undownloaded_registry_package_stays_outside() {
        assert_eq!(closure_word(true, "registry", ""), "outside");
    }

    #[test]
    fn an_undownloaded_git_package_stays_outside() {
        assert_eq!(closure_word(true, "git", ""), "outside");
    }

    #[test]
    fn a_path_package_stays_in_the_closure_without_registry_bytes() {
        assert_eq!(closure_word(true, "path", ""), "closure");
    }

    #[test]
    fn a_package_outside_the_dependency_closure_stays_outside() {
        assert_eq!(closure_word(false, "registry", "abcdef"), "outside");
        assert_eq!(closure_word(false, "path", ""), "outside");
    }
}
