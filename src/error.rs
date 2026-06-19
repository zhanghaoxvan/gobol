use crate::token::Token;

/// Formats a compiler error in rustc/gcc style with source snippet and caret.
#[derive(Clone)]
pub struct ErrorFormatter {
    file: String,
    #[allow(dead_code)]
    source: String,
    lines: Vec<String>,
}

impl ErrorFormatter {
    pub fn new(file: impl Into<String>, source: impl Into<String>) -> Self {
        let source: String = source.into();
        let lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
        ErrorFormatter {
            file: file.into(),
            source,
            lines,
        }
    }

    /// Format: `file:line:col: error: message`
    pub fn format_header(&self, line: i32, col: i32, kind: &str, msg: &str) -> String {
        if line > 0 {
            format!(
                "\u{1b}[1m{}:{}:{}: \u{1b}[31m{}\u{1b}[0m\u{1b}[1m: {}\u{1b}[0m",
                self.file, line, col + 1, kind, msg
            )
        } else {
            format!(
                "\u{1b}[1m{}: \u{1b}[31m{}\u{1b}[0m\u{1b}[1m: {}\u{1b}[0m",
                self.file, kind, msg
            )
        }
    }

    /// Format: `file:line:col: error: message` (no ANSI)
    pub fn format_header_plain(&self, line: i32, col: i32, kind: &str, msg: &str) -> String {
        if line > 0 {
            format!(
                "{}:{}:{}: {}: {}",
                self.file, line, col + 1, kind, msg
            )
        } else {
            format!("{}: {}: {}", self.file, kind, msg)
        }
    }

    /// Format source snippet with caret underline
    pub fn format_snippet(&self, line: i32, col: i32, span: usize) -> String {
        if line < 1 || line > self.lines.len() as i32 {
            return String::new();
        }
        let source_line = &self.lines[(line - 1) as usize];
        let col = col as usize;
        let span = if span == 0 { 1 } else { span };

        let mut out = String::new();
        
        // 行号宽度 + 前缀
        let line_str = format!("{}", line);
        let prefix = format!("  {} | ", line_str);
        
        // 输出源行
        out.push_str(&format!("\u{1b}[34m{}\u{1b}[0m", prefix));
        out.push_str(source_line);
        out.push('\n');
        
        // 下划线：确保前缀长度一致
        let prefix_len = prefix.len();
        let spaces_before_caret = " ".repeat(prefix_len + col);
        out.push_str(&spaces_before_caret);
        out.push_str("\u{1b}[31m");
        out.push('^');
        for _ in 1..span {
            out.push('~');
        }
        out.push_str("\u{1b}[0m\n");
        out
    }

    /// Format source snippet (no ANSI)
    pub fn format_snippet_plain(&self, line: i32, col: i32, span: usize) -> String {
        if line < 1 || line > self.lines.len() as i32 {
            return String::new();
        }
        let source_line = &self.lines[(line - 1) as usize];
        let col = col as usize;
        let span = if span == 0 { 1 } else { span };

        let mut out = String::new();

        // 行号前缀：固定格式 "  {} | "
        let line_str = format!("{}", line);
        let prefix = format!("  {} | ", line_str);

        // 输出源行
        out.push_str(&prefix);
        out.push_str(source_line);
        out.push('\n');

        // 下划线：前缀长度 + 列偏移
        let prefix_len = prefix.len();
        let spaces_before_caret = " ".repeat(prefix_len + col);
        out.push_str(&spaces_before_caret);
        out.push('^');
        for _ in 1..span {
            out.push('~');
        }
        out.push('\n');

        out
    }

    /// Full error: header + snippet
    pub fn format_error(&self, line: i32, col: i32, span: usize, kind: &str, msg: &str, use_color: bool) -> String {
        let header = if use_color {
            self.format_header(line, col, kind, msg)
        } else {
            self.format_header_plain(line, col, kind, msg)
        };
        let snippet = if line > 0 {
            if use_color {
                self.format_snippet(line, col, span)
            } else {
                self.format_snippet_plain(line, col, span)
            }
        } else {
            String::new()
        };
        if snippet.is_empty() {
            header
        } else {
            format!("{}\n{}", header, snippet)
        }
    }

    /// Format error from a token position
    pub fn format_error_at(&self, token: &Token, span: usize, kind: &str, msg: &str) -> String {
        self.format_error(token.line, token.col, span, kind, msg, true)
    }

    /// Format a "note" line
    pub fn format_note(&self, msg: &str, use_color: bool) -> String {
        if use_color {
            format!("  \u{1b}[36m= note:\u{1b}[0m {}", msg)
        } else {
            format!("  = note: {}", msg)
        }
    }
}
