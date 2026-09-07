/*
 * Gobol language definition for Highlight.js.
 *
 * Browser usage:
 *   <script src="highlight.min.js"></script>
 *   <script src="gobol.js"></script>
 *   <pre><code class="language-gobol">...</code></pre>
 *
 * CommonJS usage:
 *   const gobol = require("./gobol");
 *   hljs.registerLanguage("gobol", gobol);
 */
(function (root, factory) {
    if (typeof module === "object" && module.exports) {
        module.exports = factory;
    } else if (root.hljs) {
        root.hljs.registerLanguage("gobol", factory);
    }
}(typeof self !== "undefined" ? self : this, function (hljs) {
    const KEYWORDS = {
        keyword: [
            "as", "break", "const", "continue", "else", "enum", "export",
            "extern", "for", "from", "func", "if", "impl", "import",
            "in", "match", "operator", "return", "self", "struct",
            "lambda", "trait", "val", "var", "while"
        ],
        type: [
            "bool", "byte", "float", "int", "str", "void",
            "Array", "Option", "Ref", "Result", "TcpListener", "TcpStream",
            "Vec"
        ],
        literal: ["false", "null", "true"],
        built_in: [
            "Err", "None", "Ok", "Some", "accept", "bind", "connect",
            "close", "drop", "is_alive", "local_addr", "println",
            "recv_bytes", "recv_exact", "remote_addr", "send_bytes",
            "send_str", "set_keepalive", "set_read_timeout",
            "set_write_timeout"
        ]
    };

    const IDENTIFIER = "[A-Za-z_][A-Za-z0-9_]*";
    const ATTRIBUTE = {
        className: "meta",
        begin: /#\[/,
        end: /\]/,
        relevance: 0
    };

    const STRING = {
        className: "string",
        variants: [
            { begin: /"/, end: /"/, illegal: /\n/, contains: [{ begin: /\\[\\'"abfnrtv0xuU]/, relevance: 0 }] },
            { begin: /'/, end: /'/, illegal: /\n/, contains: [{ begin: /\\[\\'"abfnrtv0xuU]/, relevance: 0 }] }
        ]
    };
    const PRIMITIVE_TYPE = {
        className: "type",
        begin: /\b(?:bool|byte|float|int|str|void)\b/,
        relevance: 0
    };
    const FUNCTION_TYPE = {
        className: "type",
        begin: /\bfunc(?=\s*(?:<[^>]+>\s*)?\()/,
        end: /(?=[:\)])?/,
        keywords: KEYWORDS,
        contains: [PRIMITIVE_TYPE],
        relevance: 0
    };
    const LAMBDA = {
        className: "function",
        begin: /\blambda\b/,
        end: /(?=\{)/,
        keywords: KEYWORDS,
        contains: [
            {
                className: "params",
                begin: /\(/,
                end: /\)/,
                keywords: KEYWORDS,
                contains: [PRIMITIVE_TYPE, hljs.C_NUMBER_MODE]
            },
            PRIMITIVE_TYPE
        ],
        relevance: 0
    };

    return {
        name: "Gobol",
        aliases: ["gbl", "gobol"],
        case_insensitive: false,
        keywords: KEYWORDS,
        contains: [
            hljs.C_LINE_COMMENT_MODE,
            hljs.C_BLOCK_COMMENT_MODE,
            ATTRIBUTE,
            STRING,
            FUNCTION_TYPE,
            PRIMITIVE_TYPE,
            LAMBDA,
            {
                className: "string",
                begin: /@"/,
                end: /"/,
                illegal: /\n/,
                contains: [
                    { begin: /\\[\\'"abfnrtv0xuU]/, relevance: 0 },
                    { className: "subst", begin: /\{/, end: /\}/, keywords: KEYWORDS, relevance: 0 }
                ]
            },
            {
                className: "function",
                beginKeywords: "func",
                end: /[{;=>]/,
                excludeEnd: true,
                contains: [
                    { className: "title.function", begin: IDENTIFIER, relevance: 0 },
                    hljs.inherit(hljs.TITLE_MODE, { begin: IDENTIFIER, relevance: 0 }),
                    {
                        className: "params",
                        begin: /\(/,
                        end: /\)/,
                        keywords: KEYWORDS,
                        contains: [STRING, hljs.C_NUMBER_MODE]
                    }
                ],
                relevance: 0
            },
            {
                className: "class",
                beginKeywords: "struct enum trait impl",
                end: /[{;]/,
                excludeEnd: true,
                contains: [
                    { className: "title.class", begin: IDENTIFIER, relevance: 0 },
                    { className: "keyword", begin: /\bfor\b/, relevance: 0 },
                    { className: "title.class", begin: IDENTIFIER, relevance: 0 }
                ],
                relevance: 0
            },
            {
                className: "meta",
                begin: /\b(?:extern)\s+"C"/,
                relevance: 0
            },
            {
                className: "title.class",
                begin: /\b[A-Z][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*/,
                relevance: 0
            },
            {
                className: "variable",
                begin: /\bself\b/,
                relevance: 0
            },
            {
                className: "operator",
                begin: /::|->|=>|==|!=|<=|>=|&&|\|\||[+\-*\/%<>=!&|]/,
                relevance: 0
            },
            hljs.NUMBER_MODE
        ]
    };
}));
