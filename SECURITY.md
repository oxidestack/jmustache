# Security policy

Please report a vulnerability privately, so that it can be fixed before it is public: on the repository's **Security**
tab choose **Report a vulnerability**, or use
[the form](https://github.com/oxidestack/jmustache/security/advisories/new). Only the maintainers see it. Do not open
a public issue, discussion or pull request for one.

Say which version or commit, the template and data that trigger it, and what an attacker gets (a read outside the
template, a lambda or reflection reached that should not be, memory or CPU exhausted). The fix is released with a GitHub
security advisory and credits you unless you prefer not to be named.

## Supported versions

Security fixes go into the latest release.

## Scope

In scope: the template compiler and renderer. A template is code that chooses what to read from its data: rendering a
template you do not trust is a deployment decision, but one that reaches what the engine documents as out of reach, or
that exhausts memory or CPU out of proportion to its size, is a bug worth reporting. A difference from JMustache's
output is a bug, to report as one. A vulnerability in JMustache itself belongs to the
[JMustache project](https://github.com/samskivert/jmustache); if it also affects this crate, report it here too.
