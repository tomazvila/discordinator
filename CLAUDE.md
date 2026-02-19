# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Feature Implementation

**Before implementing any new feature, read `REQUIREMENTS.md` for specifications:**

- Feature checklists and phased implementation (Phase 1 MVP, Phase 2 Enhanced, Phase 3 Future)
- Data model definitions (entities, fields, relationships)
- API endpoint specifications
- Visibility and privacy rules
- User roles and permissions
- Security requirements

When implementing a feature:
1. Find the feature in `REQUIREMENTS.md` and understand its full specification
2. Follow the data model definitions for entity structure
3. Use the API design patterns specified
4. Implement visibility/privacy rules as documented
5. Mark the feature checkbox in `REQUIREMENTS.md` when complete

## Codemaps

**Before making changes, read the relevant codemap(s) in `.codemaps/` to understand the architecture:**

- `.codemaps/overview.md` - System architecture, component interactions, request flows
- `.codemaps/backend.md` - Backend layers, API endpoints, services, entity relationships
- `.codemaps/frontend.md` - Component tree, state management, API client patterns
- `.codemaps/image-worker.md` - Async processing pipeline, RabbitMQ configuration

When working on a feature or bug:
1. Read `overview.md` first for context
2. Read the component-specific codemap (backend/frontend/image-worker)
3. Follow the patterns and conventions documented there

## Test-Driven Development (Required)

**All changes must follow TDD:**
1. Write a test that fails (verifies the bug exists or the feature is missing)
2. Run the test to confirm it fails
3. Implement the minimum code to make the test pass
4. Run the test to confirm it passes
5. Refactor if needed (keeping tests green)

Do not implement code without a corresponding failing test first.

## Project Overview

## Development Commands

Everything MUST be done in `nix develop` enviroment enabled by nix flake. Not a single dependency must be installed globally in this machine.

### Infrastructure

### Backend

### E2E Tests

### Full Stack Start Order

## Architecture

### Backend Structure

### Key Patterns

## Testing

## Environment

Uses Nix flakes for reproducible dev environment. The `flake.nix` is located at the project root level.

## Key Entities

