# Contributing to superseedr

Thank you for your interest in helping improve superseedr!

You do not need programming experience to contribute. Some of the most helpful contributions are bug reports, feature ideas, and general feedback.

## 🐛 Report a Bug

If something doesn't work as expected, please open a GitHub issue and include:

- A clear title describing the problem
- What you expected to happen
- What actually happened
- Steps to reproduce the issue
- Your environment (OS, version, Docker or native, etc.)
- Any relevant logs or error messages

Before creating a new issue, please search [[existing issues](https://github.com/Jagalite/superseedr/issues)](https://github.com/Jagalite/superseedr/issues) and [[discussions](https://github.com/Jagalite/superseedr/discussions)](https://github.com/Jagalite/superseedr/discussions) to avoid duplicates or find existing solutions.

## 💡 Suggest a Feature or Idea

Have an idea to improve superseedr?

Before creating a new issue, please search [[existing issues](https://github.com/Jagalite/superseedr/issues)](https://github.com/Jagalite/superseedr/issues) and [[discussions](https://github.com/Jagalite/superseedr/discussions)](https://github.com/Jagalite/superseedr/discussions) to see if your idea has already been proposed or discussed.

You can open a GitHub issue and describe:

- What problem you're trying to solve
- Your suggested solution or idea
- Why it would be useful to users

Even rough or incomplete ideas are welcome.

## 📝 Help Improve Documentation

You can contribute by:

- Reporting confusing or outdated docs
- Suggesting clearer explanations
- Proposing examples or setup guides
- Improving the README, FAQ, or other documentation files

## 🔒 Report a Security Vulnerability

If you discover a security vulnerability, **please do not open a public issue.**

Instead:
1. Contact the maintainers privately (use GitHub Security Advisory or email)
2. Include a detailed description of the vulnerability
3. Provide steps to reproduce if possible
4. Allow time for a fix before public disclosure

We take security seriously and will respond promptly.

## Guidelines for All Contributions

### ✅ General Guidelines

- Be respectful and constructive
- Keep discussions on-topic
- Provide as much relevant detail as possible
- For existing issues or discussions, you can "bump" them by adding a comment if you have new information, want to express increased urgency, or can provide additional details/context

---

## 🧑‍💻 Contributing Code (for developers)

### Development Environment Setup

**Prerequisites:**
- Rust toolchain (latest stable version)
- Docker and Docker Compose (for Docker-related changes)
- A terminal with Unicode support (Windows Terminal, iTerm2, or modern Linux terminals)
- Git

**Quick Start:**
```bash
# Fork the repository on GitHub first, then clone your fork
git clone https://github.com/YOUR_USERNAME/superseedr.git
cd superseedr

# Build the project
cargo build

# Run tests
cargo test

# Run locally
cargo run
```

**For Docker development:**
```bash
# Build the Docker image locally
docker build -t superseedr-dev .

# Test standalone Docker setup
docker compose -f docker-compose.standalone.yml up

# Test with Gluetun (requires .env and .gluetun.env configuration)
docker compose up
```

### Code Style & Formatting

- Run `cargo fmt` before committing to format your code
- Ensure `cargo clippy` passes without warnings
- Follow Rust naming conventions:
  - `snake_case` for functions and variables
  - `PascalCase` for types and structs
  - `SCREAMING_SNAKE_CASE` for constants
- Add documentation comments (`///`) for public APIs and complex logic
- Keep line length reasonable (suggested 100 characters, but not strict)

### Testing

Superseedr uses multiple testing strategies to ensure reliability:

**Unit Tests:**
```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture
```

**Model-Based Fuzzing:**
The project uses model-based testing for protocol correctness. Fuzzing tests run nightly via GitHub Actions to verify BitTorrent protocol implementation.

**Manual Testing:**
- Test with real torrents in a safe environment (use legal content like Linux ISOs)
- Verify VPN integration with Gluetun if modifying networking code
- Check TUI rendering in different terminal emulators (iTerm2, Windows Terminal, Alacritty, etc.)
- Test in both light and dark terminal colour schemes
- Verify keyboard controls work as expected

**When contributing code:**
- Add unit tests for new functionality
- Update existing tests if changing behavior
- Ensure all tests pass before submitting a PR

### Working on the TUI

Superseedr uses [[Ratatui](https://ratatui.rs/)](https://ratatui.rs/) for the terminal interface.

**Testing UI changes:**
- Run the app locally: `cargo run`
- Test in different terminal sizes (resize your terminal window)
- Verify rendering in multiple terminal emulators
- Check that animations remain performant (1-60 FPS target)
- Ensure colour schemes work in both light and dark modes

**UI Guidelines:**
- Keep animations performant and smooth
- Ensure all features are keyboard-accessible (no mouse-only features)
- Maintain consistency with existing keybinding patterns
- Follow the existing visual style and layout conventions
- Test with the minimum supported terminal size

### Docker & VPN Changes

When modifying Docker setup or VPN integration:

- Test with both Gluetun and standalone configurations
- Verify port forwarding works correctly
- Check that dynamic port reloading functions as expected
- Update `.env.example` and `.gluetun.env.example` if adding new configuration variables
- Test with at least one VPN provider if possible
- Document any new environment variables in the README

### Private Tracker Support

Superseedr supports private tracker builds that disable DHT and PEX.

When contributing:
- Ensure changes don't break private tracker mode
- Test both public and private tracker configurations if modifying protocol behavior
- Respect the privacy and security requirements of private trackers

### Continuous Integration

All PRs must pass automated checks:

- ✅ Rust build and compilation
- ✅ All unit tests
- ✅ Clippy lints (no warnings)
- ✅ Code formatting check (`cargo fmt`)
- ✅ Model-based fuzzing (runs nightly)

Check the Actions tab on your PR to see CI results. Fix any failures before requesting review.

### Branch Naming Conventions

Create descriptive branch names following these patterns:

- Feature: `feature/add-upnp-support`
- Bug fix: `fix/port-reload-crash`
- Documentation: `docs/update-contributing-guide`
- Refactoring: `refactor/simplify-peer-manager`
- Performance: `perf/optimize-piece-selection`

### Contribution Workflow

1. **Discuss your changes first:** Before writing extensive code, please open an issue to discuss your proposed changes, especially regarding:
   - **Importance/Priority:** Why is this change important now?
   - **Protocol/Architecture Compliance:** How does it fit into the existing design and protocols?
   - **Security Implications:** Are there any security considerations or impacts?

2. **Fork the repository** (if you haven't already)

3. **Clone your fork locally:**
   ```bash
   git clone https://github.com/YOUR_USERNAME/superseedr.git
   cd superseedr
   ```

4. **Create a new branch** with a descriptive name:
   ```bash
   git checkout -b feature/your-feature-name
   ```

5. **Make your changes:**
   - Write clean, documented code
   - Follow existing code style and conventions
   - Add tests for new functionality

6. **Test your changes:**
   ```bash
   cargo build
   cargo test
   cargo clippy
   cargo fmt --check
   ```

7. **Commit your changes:**
   ```bash
   git add .
   git commit -m "Add feature: brief description"
   ```
   - Use clear, descriptive commit messages
   - Reference issue numbers when applicable (e.g., "Fix #123: resolve port binding issue")

8. **Push to your fork:**
   ```bash
   git push origin feature/your-feature-name
   ```

9. **Open a Pull Request** with:
   - A clear title describing the change
   - Description of what changed and why
   - Link to related issues (e.g., "Fixes #123", "Relates to #456")
   - Screenshots or demos for UI changes
   - Notes on testing performed

### Pull Request Guidelines

- Keep changes focused and scoped to a single feature or fix
- Describe what changed and why in the PR description
- Link related issues if applicable
- Respond to review feedback promptly and constructively
- Be patient - maintainers review PRs as time permits
- Update your PR if the main branch has moved forward

## 🙏 Recognition

All contributors will be acknowledged in release notes. Thank you for making superseedr better!

## Additional Resources

- 📖 [FAQ](FAQ.md) - Common questions and answers
- 🗺️ [Roadmap](ROADMAP.md) - Future plans and features
- 📜 [Changelog](CHANGELOG.md) - Recent changes and version history
- 🤝 [Code of Conduct](CODE_OF_CONDUCT.md) - Community standards
- 💬 [[Discussions](https://github.com/Jagalite/superseedr/discussions)](https://github.com/Jagalite/superseedr/discussions) - General questions and ideas
- 📚 [[Ratatui Documentation](https://ratatui.rs/)](https://ratatui.rs/) - TUI framework reference

## Questions?

If you're unsure about anything, don't hesitate to:
- Ask in [[Discussions](https://github.com/Jagalite/superseedr/discussions)](https://github.com/Jagalite/superseedr/discussions)
- Comment on a relevant issue
- Reach out to maintainers

We're here to help and appreciate your interest in contributing! 🚀
