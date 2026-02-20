use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

/// Login method selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    Token,
    EmailPassword,
    QrCode,
}

/// Active input field in the login form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginField {
    Token,
    Email,
    Password,
    MfaCode,
}

/// Status of the login process.
#[derive(Debug, Clone, PartialEq)]
pub enum LoginStatus {
    Idle,
    Validating,
    MfaRequired { ticket: String },
    Success(String),
    Error(String),
}

/// Complete login screen state.
#[derive(Debug, Clone)]
pub struct LoginState {
    pub method: LoginMethod,
    pub active_field: LoginField,
    pub token_input: String,
    pub email_input: String,
    pub password_input: String,
    pub mfa_input: String,
    pub status: LoginStatus,
    pub cursor_visible: bool,
}

impl Default for LoginState {
    fn default() -> Self {
        Self {
            method: LoginMethod::Token,
            active_field: LoginField::Token,
            token_input: String::new(),
            email_input: String::new(),
            password_input: String::new(),
            mfa_input: String::new(),
            status: LoginStatus::Idle,
            cursor_visible: true,
        }
    }
}

impl LoginState {
    /// Switch to a different login method. Resets the active field accordingly.
    pub fn set_method(&mut self, method: LoginMethod) {
        self.method = method;
        self.active_field = match method {
            LoginMethod::Token => LoginField::Token,
            LoginMethod::EmailPassword => LoginField::Email,
            LoginMethod::QrCode => LoginField::Token, // QR has no input fields
        };
        self.status = LoginStatus::Idle;
    }

    /// Cycle to the next login method.
    pub fn next_method(&mut self) {
        let next = match self.method {
            LoginMethod::Token => LoginMethod::EmailPassword,
            LoginMethod::EmailPassword => LoginMethod::QrCode,
            LoginMethod::QrCode => LoginMethod::Token,
        };
        self.set_method(next);
    }

    /// Cycle to the previous login method.
    pub fn prev_method(&mut self) {
        let prev = match self.method {
            LoginMethod::Token => LoginMethod::QrCode,
            LoginMethod::EmailPassword => LoginMethod::Token,
            LoginMethod::QrCode => LoginMethod::EmailPassword,
        };
        self.set_method(prev);
    }

    /// Move focus to the next input field (within current method).
    pub fn next_field(&mut self) {
        self.active_field = match (&self.method, &self.active_field, &self.status) {
            (LoginMethod::EmailPassword, LoginField::Email, _) => LoginField::Password,
            (LoginMethod::EmailPassword, LoginField::Password, LoginStatus::MfaRequired { .. }) => {
                LoginField::MfaCode
            }
            (LoginMethod::EmailPassword, LoginField::MfaCode, _) => LoginField::Email,
            (LoginMethod::EmailPassword, LoginField::Password, _) => LoginField::Email,
            _ => self.active_field,
        };
    }

    /// Get a reference to the currently active input string.
    pub fn active_input(&self) -> &str {
        match self.active_field {
            LoginField::Token => &self.token_input,
            LoginField::Email => &self.email_input,
            LoginField::Password => &self.password_input,
            LoginField::MfaCode => &self.mfa_input,
        }
    }

    /// Get a mutable reference to the currently active input string.
    pub fn active_input_mut(&mut self) -> &mut String {
        match self.active_field {
            LoginField::Token => &mut self.token_input,
            LoginField::Email => &mut self.email_input,
            LoginField::Password => &mut self.password_input,
            LoginField::MfaCode => &mut self.mfa_input,
        }
    }

    /// Type a character into the active input field.
    pub fn type_char(&mut self, c: char) {
        self.active_input_mut().push(c);
    }

    /// Delete the last character from the active input field.
    pub fn backspace(&mut self) {
        self.active_input_mut().pop();
    }

    /// Clear the active input field.
    pub fn clear_active_input(&mut self) {
        self.active_input_mut().clear();
    }

    /// Get masked display string for sensitive fields (password, token).
    pub fn masked_display(&self, field: LoginField) -> String {
        let input = match field {
            LoginField::Token => &self.token_input,
            LoginField::Password => &self.password_input,
            LoginField::Email => return self.email_input.clone(),
            LoginField::MfaCode => return self.mfa_input.clone(),
        };
        "*".repeat(input.len())
    }

    /// Check if the current form is ready to submit.
    pub fn can_submit(&self) -> bool {
        match (&self.method, &self.status) {
            (LoginMethod::Token, LoginStatus::Idle | LoginStatus::Error(_)) => {
                !self.token_input.is_empty()
            }
            (LoginMethod::EmailPassword, LoginStatus::Idle | LoginStatus::Error(_)) => {
                !self.email_input.is_empty() && !self.password_input.is_empty()
            }
            (LoginMethod::EmailPassword, LoginStatus::MfaRequired { .. }) => {
                !self.mfa_input.is_empty()
            }
            _ => false,
        }
    }
}

/// Login screen widget. Renders the login form based on LoginState.
pub struct LoginScreen<'a> {
    pub state: &'a LoginState,
    pub qr_lines: Option<&'a [String]>,
}

impl<'a> LoginScreen<'a> {
    pub fn new(state: &'a LoginState) -> Self {
        Self {
            state,
            qr_lines: None,
        }
    }

    pub fn with_qr_lines(mut self, lines: &'a [String]) -> Self {
        self.qr_lines = Some(lines);
        self
    }
}

impl Widget for LoginScreen<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        // Clear the area
        Clear.render(area, buf);

        // Main block
        let block = Block::default()
            .title(" Discordinator Login ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 3 || inner.width < 20 {
            return;
        }

        // Layout: tabs | content | status
        let chunks = Layout::vertical([
            Constraint::Length(2), // Method tabs
            Constraint::Min(5),   // Form content
            Constraint::Length(2), // Status bar
        ])
        .split(inner);

        // Render method tabs
        self.render_tabs(chunks[0], buf);

        // Render form content
        match self.state.method {
            LoginMethod::Token => self.render_token_form(chunks[1], buf),
            LoginMethod::EmailPassword => self.render_email_form(chunks[1], buf),
            LoginMethod::QrCode => self.render_qr_code(chunks[1], buf),
        }

        // Render status
        self.render_status(chunks[2], buf);
    }
}

impl LoginScreen<'_> {
    fn render_tabs(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let tabs = [
            ("1: Token", LoginMethod::Token),
            ("2: Email/Pass", LoginMethod::EmailPassword),
            ("3: QR Code", LoginMethod::QrCode),
        ];

        let spans: Vec<Span> = tabs
            .iter()
            .flat_map(|(label, method)| {
                let style = if *method == self.state.method {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                vec![Span::styled(*label, style), Span::raw("  ")]
            })
            .collect();

        let line = Line::from(spans);
        let para = Paragraph::new(line).alignment(Alignment::Center);
        para.render(area, buf);
    }

    fn render_token_form(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let chunks = Layout::vertical([
            Constraint::Length(1), // Label
            Constraint::Length(3), // Input
            Constraint::Min(0),   // Spacer
        ])
        .split(area);

        let label = Paragraph::new("Paste your Discord token:")
            .style(Style::default().fg(Color::White));
        label.render(chunks[0], buf);

        let display_text = self.state.masked_display(LoginField::Token);
        let input_style = if self.state.active_field == LoginField::Token {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let input = Paragraph::new(display_text)
            .style(input_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if self.state.active_field == LoginField::Token {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            );
        input.render(chunks[1], buf);
    }

    fn render_email_form(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let show_mfa = matches!(self.state.status, LoginStatus::MfaRequired { .. });

        let constraints = if show_mfa {
            vec![
                Constraint::Length(1), // Email label
                Constraint::Length(3), // Email input
                Constraint::Length(1), // Password label
                Constraint::Length(3), // Password input
                Constraint::Length(1), // MFA label
                Constraint::Length(3), // MFA input
                Constraint::Min(0),   // Spacer
            ]
        } else {
            vec![
                Constraint::Length(1), // Email label
                Constraint::Length(3), // Email input
                Constraint::Length(1), // Password label
                Constraint::Length(3), // Password input
                Constraint::Min(0),   // Spacer
            ]
        };

        let chunks = Layout::vertical(constraints).split(area);

        // Email
        let label = Paragraph::new("Email:").style(Style::default().fg(Color::White));
        label.render(chunks[0], buf);

        let email_style = if self.state.active_field == LoginField::Email {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let email_input = Paragraph::new(self.state.email_input.as_str())
            .style(email_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if self.state.active_field == LoginField::Email {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            );
        email_input.render(chunks[1], buf);

        // Password
        let label = Paragraph::new("Password:").style(Style::default().fg(Color::White));
        label.render(chunks[2], buf);

        let pass_display = self.state.masked_display(LoginField::Password);
        let pass_style = if self.state.active_field == LoginField::Password {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let pass_input = Paragraph::new(pass_display)
            .style(pass_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if self.state.active_field == LoginField::Password {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            );
        pass_input.render(chunks[3], buf);

        // MFA field (only if needed)
        if show_mfa {
            let label = Paragraph::new("2FA Code:").style(Style::default().fg(Color::Yellow));
            label.render(chunks[4], buf);

            let mfa_style = if self.state.active_field == LoginField::MfaCode {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let mfa_input = Paragraph::new(self.state.mfa_input.as_str())
                .style(mfa_style)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(if self.state.active_field == LoginField::MfaCode {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                );
            mfa_input.render(chunks[5], buf);
        }
    }

    fn render_qr_code(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if let Some(qr_lines) = self.qr_lines {
            let lines: Vec<Line> = qr_lines.iter().map(|l| Line::raw(l.as_str())).collect();
            let qr = Paragraph::new(lines).alignment(Alignment::Center);
            qr.render(area, buf);
        } else {
            let text = Paragraph::new("Generating QR code...")
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center);
            text.render(area, buf);
        }
    }

    fn render_status(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let (text, style) = match &self.state.status {
            LoginStatus::Idle => (
                "Press Enter to submit | Tab to switch fields | 1/2/3 to switch method",
                Style::default().fg(Color::DarkGray),
            ),
            LoginStatus::Validating => ("Validating...", Style::default().fg(Color::Yellow)),
            LoginStatus::MfaRequired { .. } => (
                "Enter your 2FA code and press Enter",
                Style::default().fg(Color::Yellow),
            ),
            LoginStatus::Success(_) => (
                "Login successful!",
                Style::default().fg(Color::Green),
            ),
            LoginStatus::Error(msg) => {
                let para = Paragraph::new(msg.as_str())
                    .style(Style::default().fg(Color::Red))
                    .alignment(Alignment::Center);
                para.render(area, buf);
                return;
            }
        };

        let para = Paragraph::new(text)
            .style(style)
            .alignment(Alignment::Center);
        para.render(area, buf);
    }
}

// validate_token_via_gateway lives in crate::auth (not here — presentation must not import infrastructure)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DiscordConfig;
    use ratatui::{backend::TestBackend, Terminal};

    // === Task 38: LoginState tests ===

    #[test]
    fn login_state_default() {
        let state = LoginState::default();
        assert_eq!(state.method, LoginMethod::Token);
        assert_eq!(state.active_field, LoginField::Token);
        assert!(state.token_input.is_empty());
        assert!(state.email_input.is_empty());
        assert!(state.password_input.is_empty());
        assert!(state.mfa_input.is_empty());
        assert_eq!(state.status, LoginStatus::Idle);
    }

    #[test]
    fn login_state_set_method_token() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::EmailPassword);
        assert_eq!(state.method, LoginMethod::EmailPassword);
        assert_eq!(state.active_field, LoginField::Email);

        state.set_method(LoginMethod::Token);
        assert_eq!(state.method, LoginMethod::Token);
        assert_eq!(state.active_field, LoginField::Token);
    }

    #[test]
    fn login_state_set_method_qr() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::QrCode);
        assert_eq!(state.method, LoginMethod::QrCode);
    }

    #[test]
    fn login_state_next_method_cycles() {
        let mut state = LoginState::default();
        assert_eq!(state.method, LoginMethod::Token);

        state.next_method();
        assert_eq!(state.method, LoginMethod::EmailPassword);

        state.next_method();
        assert_eq!(state.method, LoginMethod::QrCode);

        state.next_method();
        assert_eq!(state.method, LoginMethod::Token);
    }

    #[test]
    fn login_state_prev_method_cycles() {
        let mut state = LoginState::default();
        state.prev_method();
        assert_eq!(state.method, LoginMethod::QrCode);

        state.prev_method();
        assert_eq!(state.method, LoginMethod::EmailPassword);

        state.prev_method();
        assert_eq!(state.method, LoginMethod::Token);
    }

    #[test]
    fn login_state_next_field_email_password() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::EmailPassword);
        assert_eq!(state.active_field, LoginField::Email);

        state.next_field();
        assert_eq!(state.active_field, LoginField::Password);

        state.next_field(); // Wraps back to email (no MFA)
        assert_eq!(state.active_field, LoginField::Email);
    }

    #[test]
    fn login_state_next_field_with_mfa() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::EmailPassword);
        state.status = LoginStatus::MfaRequired {
            ticket: "ticket".to_string(),
        };
        state.active_field = LoginField::Password;

        state.next_field();
        assert_eq!(state.active_field, LoginField::MfaCode);

        state.next_field();
        assert_eq!(state.active_field, LoginField::Email);
    }

    #[test]
    fn login_state_type_char_and_backspace() {
        let mut state = LoginState::default();
        state.type_char('a');
        state.type_char('b');
        state.type_char('c');
        assert_eq!(state.token_input, "abc");

        state.backspace();
        assert_eq!(state.token_input, "ab");

        state.clear_active_input();
        assert_eq!(state.token_input, "");
    }

    #[test]
    fn login_state_type_into_email_field() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::EmailPassword);
        state.type_char('u');
        state.type_char('@');
        assert_eq!(state.email_input, "u@");
        assert_eq!(state.token_input, ""); // Other fields unchanged
    }

    #[test]
    fn login_state_active_input_returns_correct_field() {
        let mut state = LoginState::default();
        state.token_input = "token123".to_string();
        assert_eq!(state.active_input(), "token123");

        state.set_method(LoginMethod::EmailPassword);
        state.email_input = "user@test.com".to_string();
        assert_eq!(state.active_input(), "user@test.com");

        state.active_field = LoginField::Password;
        state.password_input = "secret".to_string();
        assert_eq!(state.active_input(), "secret");
    }

    #[test]
    fn login_state_masked_display() {
        let mut state = LoginState::default();
        state.token_input = "abc123".to_string();
        assert_eq!(state.masked_display(LoginField::Token), "******");

        state.password_input = "pass".to_string();
        assert_eq!(state.masked_display(LoginField::Password), "****");

        state.email_input = "user@test.com".to_string();
        assert_eq!(state.masked_display(LoginField::Email), "user@test.com");

        state.mfa_input = "123456".to_string();
        assert_eq!(state.masked_display(LoginField::MfaCode), "123456");
    }

    #[test]
    fn login_state_can_submit_token() {
        let mut state = LoginState::default();
        assert!(!state.can_submit()); // Empty token

        state.token_input = "abc".to_string();
        assert!(state.can_submit());

        state.status = LoginStatus::Validating;
        assert!(!state.can_submit()); // Already validating
    }

    #[test]
    fn login_state_can_submit_email_password() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::EmailPassword);
        assert!(!state.can_submit()); // Both empty

        state.email_input = "user@test.com".to_string();
        assert!(!state.can_submit()); // Password empty

        state.password_input = "pass".to_string();
        assert!(state.can_submit()); // Both filled
    }

    #[test]
    fn login_state_can_submit_mfa() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::EmailPassword);
        state.status = LoginStatus::MfaRequired {
            ticket: "ticket".to_string(),
        };
        assert!(!state.can_submit()); // MFA code empty

        state.mfa_input = "123456".to_string();
        assert!(state.can_submit());
    }

    #[test]
    fn login_state_can_submit_after_error() {
        let mut state = LoginState::default();
        state.token_input = "abc".to_string();
        state.status = LoginStatus::Error("previous error".to_string());
        assert!(state.can_submit()); // Can retry after error
    }

    #[test]
    fn login_status_variants() {
        let idle = LoginStatus::Idle;
        let validating = LoginStatus::Validating;
        let mfa = LoginStatus::MfaRequired {
            ticket: "t".to_string(),
        };
        let success = LoginStatus::Success("token".to_string());
        let error = LoginStatus::Error("bad".to_string());

        assert_eq!(idle, LoginStatus::Idle);
        assert_eq!(validating, LoginStatus::Validating);
        assert_ne!(idle, validating);
        assert_ne!(mfa, success);
        assert_ne!(error, idle);
    }

    #[test]
    fn login_set_method_resets_status() {
        let mut state = LoginState::default();
        state.status = LoginStatus::Error("old error".to_string());
        state.set_method(LoginMethod::EmailPassword);
        assert_eq!(state.status, LoginStatus::Idle);
    }

    // === Task 38: LoginScreen widget rendering tests ===

    fn render_login_screen(state: &LoginState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let screen = LoginScreen::new(state);
                f.render_widget(screen, f.area());
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut output = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                output.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            output.push('\n');
        }
        output
    }

    #[test]
    fn login_screen_renders_title() {
        let state = LoginState::default();
        let output = render_login_screen(&state, 60, 20);
        assert!(
            output.contains("Discordinator Login"),
            "Should show title: {}",
            output
        );
    }

    #[test]
    fn login_screen_renders_token_tab_active() {
        let state = LoginState::default();
        let output = render_login_screen(&state, 60, 20);
        assert!(output.contains("1: Token"), "Should show token tab");
        assert!(output.contains("2: Email/Pass"), "Should show email tab");
        assert!(output.contains("3: QR Code"), "Should show QR tab");
    }

    #[test]
    fn login_screen_renders_token_form() {
        let state = LoginState::default();
        let output = render_login_screen(&state, 60, 20);
        assert!(
            output.contains("Paste your Discord token"),
            "Should show token prompt: {}",
            output
        );
    }

    #[test]
    fn login_screen_renders_email_form() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::EmailPassword);
        let output = render_login_screen(&state, 60, 20);
        assert!(output.contains("Email"), "Should show email label: {}", output);
        assert!(
            output.contains("Password"),
            "Should show password label: {}",
            output
        );
    }

    #[test]
    fn login_screen_renders_qr_placeholder() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::QrCode);
        let output = render_login_screen(&state, 60, 20);
        assert!(
            output.contains("Generating QR code"),
            "Should show QR placeholder: {}",
            output
        );
    }

    #[test]
    fn login_screen_renders_qr_with_lines() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::QrCode);
        let qr_lines = vec!["##  ##".to_string(), "  ##  ".to_string()];

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let screen = LoginScreen::new(&state).with_qr_lines(&qr_lines);
                f.render_widget(screen, f.area());
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut output = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                output.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            output.push('\n');
        }
        assert!(
            output.contains("##  ##"),
            "Should render QR lines: {}",
            output
        );
    }

    #[test]
    fn login_screen_renders_error_status() {
        let mut state = LoginState::default();
        state.status = LoginStatus::Error("Invalid token".to_string());
        let output = render_login_screen(&state, 60, 20);
        assert!(
            output.contains("Invalid token"),
            "Should show error message: {}",
            output
        );
    }

    #[test]
    fn login_screen_renders_validating_status() {
        let mut state = LoginState::default();
        state.status = LoginStatus::Validating;
        let output = render_login_screen(&state, 60, 20);
        assert!(
            output.contains("Validating"),
            "Should show validating status: {}",
            output
        );
    }

    #[test]
    fn login_screen_renders_success_status() {
        let mut state = LoginState::default();
        state.status = LoginStatus::Success("token123".to_string());
        let output = render_login_screen(&state, 60, 20);
        assert!(
            output.contains("Login successful"),
            "Should show success: {}",
            output
        );
    }

    #[test]
    fn login_screen_renders_mfa_form() {
        let mut state = LoginState::default();
        state.set_method(LoginMethod::EmailPassword);
        state.status = LoginStatus::MfaRequired {
            ticket: "ticket123".to_string(),
        };
        let output = render_login_screen(&state, 60, 20);
        assert!(
            output.contains("2FA Code"),
            "Should show 2FA code field: {}",
            output
        );
    }

    #[test]
    fn login_screen_renders_masked_token() {
        let mut state = LoginState::default();
        state.token_input = "secret_token".to_string();
        let output = render_login_screen(&state, 60, 20);
        assert!(
            output.contains("************"),
            "Token should be masked: {}",
            output
        );
        assert!(
            !output.contains("secret_token"),
            "Raw token should NOT appear: {}",
            output
        );
    }

    #[test]
    fn login_screen_small_terminal() {
        // Should not panic with very small terminal
        let state = LoginState::default();
        let _output = render_login_screen(&state, 10, 5);
        // Just verify it doesn't panic
    }

    // === Task 38: validate_token_via_gateway tests ===

    #[tokio::test]
    async fn validate_valid_token_via_mock_gateway() {
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws.split();

            // Send HELLO
            let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 45000}});
            write
                .send(Message::Text(hello.to_string().into()))
                .await
                .unwrap();

            // Read IDENTIFY
            use futures_util::StreamExt;
            let _ = read.next().await;

            // Send READY (valid token)
            let ready = serde_json::json!({
                "op": 0, "t": "READY", "s": 1,
                "d": {
                    "session_id": "test_session",
                    "resume_gateway_url": "wss://resume.test",
                    "guilds": [], "private_channels": [],
                    "user": {"id": "1", "username": "test"},
                    "read_state": [], "relationships": []
                }
            });
            write
                .send(Message::Text(ready.to_string().into()))
                .await
                .unwrap();

            // Read close and handle gracefully
            let _ = read.next().await;
        });

        let config = DiscordConfig::default();
        let url = format!("ws://{}", addr);
        let result = crate::auth::validate_token_via_gateway("valid_token", &url, &config).await;

        let _ = server.await;
        assert!(result.is_ok());
        assert!(result.unwrap(), "Valid token should return true");
    }

    #[tokio::test]
    async fn validate_invalid_token_via_mock_gateway() {
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws.split();

            // Send HELLO
            let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 45000}});
            write
                .send(Message::Text(hello.to_string().into()))
                .await
                .unwrap();

            // Read IDENTIFY
            use futures_util::StreamExt;
            let _ = read.next().await;

            // Send InvalidSession (invalid token)
            let invalid = serde_json::json!({"op": 9, "d": false});
            write
                .send(Message::Text(invalid.to_string().into()))
                .await
                .unwrap();

            let _ = read.next().await;
        });

        let config = DiscordConfig::default();
        let url = format!("ws://{}", addr);
        let result = crate::auth::validate_token_via_gateway("invalid_token", &url, &config).await;

        let _ = server.await;
        assert!(result.is_ok());
        assert!(!result.unwrap(), "Invalid token should return false");
    }

    #[tokio::test]
    async fn validate_token_connection_failure() {
        let config = DiscordConfig::default();
        // Connect to a port that's not listening
        let result =
            crate::auth::validate_token_via_gateway("token", "ws://127.0.0.1:1", &config).await;
        assert!(result.is_err(), "Should fail when gateway is unreachable");
    }

    #[tokio::test]
    async fn validate_token_sends_correct_identify() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (identify_tx, mut identify_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(1);

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws.split();

            // Send HELLO
            let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 45000}});
            write
                .send(Message::Text(hello.to_string().into()))
                .await
                .unwrap();

            // Read IDENTIFY and capture it
            if let Some(Ok(msg)) = read.next().await {
                let text = msg.into_text().unwrap();
                let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
                let _ = identify_tx.send(payload).await;
            }

            // Send READY
            let ready = serde_json::json!({
                "op": 0, "t": "READY", "s": 1,
                "d": {
                    "session_id": "s", "resume_gateway_url": "wss://r",
                    "guilds": [], "private_channels": [],
                    "user": {"id": "1", "username": "t"},
                    "read_state": [], "relationships": []
                }
            });
            write
                .send(Message::Text(ready.to_string().into()))
                .await
                .unwrap();

            let _ = read.next().await;
        });

        let config = DiscordConfig::default();
        let url = format!("ws://{}", addr);
        let _ = crate::auth::validate_token_via_gateway("my_token", &url, &config).await;

        let identify = identify_rx.recv().await.unwrap();
        let _ = server.await;

        assert_eq!(identify["op"], 2);
        assert_eq!(identify["d"]["token"], "my_token");
        assert!(
            identify["d"]["intents"].is_null(),
            "User IDENTIFY must not have intents"
        );
        assert_eq!(identify["d"]["properties"]["os"], "Mac OS X");
        assert_eq!(identify["d"]["properties"]["browser"], "Chrome");
    }

    // === Task 40: QR auth WebSocket protocol tests ===

    #[tokio::test]
    async fn qr_auth_websocket_handshake() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (init_tx, mut init_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(1);

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws.split();

            // Send hello
            let hello = serde_json::json!({
                "op": "hello",
                "heartbeat_interval": 41250,
                "timeout_ms": 120000
            });
            write
                .send(Message::Text(hello.to_string().into()))
                .await
                .unwrap();

            // Read init message
            if let Some(Ok(msg)) = read.next().await {
                let text = msg.into_text().unwrap();
                let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
                let _ = init_tx.send(payload).await;
            }

            let _ = write.send(Message::Close(None)).await;
        });

        let session = crate::auth::QrAuthSession::new().unwrap();
        let url = format!("ws://{}", addr);

        // Connect and do the handshake manually
        let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let (mut write, mut read) = ws_stream.split();

        // Read hello
        let hello_msg = read.next().await.unwrap().unwrap();
        let hello_text = hello_msg.into_text().unwrap();
        let hello_payload: serde_json::Value = serde_json::from_str(&hello_text).unwrap();
        let parsed = crate::auth::parse_qr_auth_message(&hello_payload);
        assert_eq!(
            parsed,
            crate::auth::QrAuthMessage::Hello {
                heartbeat_interval: 41250,
                timeout_ms: 120000,
            }
        );

        // Send init
        let init_msg = crate::auth::build_qr_auth_init(&session.encoded_public_key());
        write
            .send(Message::Text(init_msg.to_string().into()))
            .await
            .unwrap();

        // Verify server received init
        let captured_init = init_rx.recv().await.unwrap();
        assert_eq!(captured_init["op"], "init");
        assert!(!captured_init["encoded_public_key"]
            .as_str()
            .unwrap()
            .is_empty());

        let _ = server.await;
    }
}
