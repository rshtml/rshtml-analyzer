;((source_file) @injection.content
;  (#not-match? @injection.content "comment_block")
;  (#set! injection.language "html")
;  (#set! injection.include-children)
;  (#set! injection.combine))

;(html_text
;  text: (source_file) @injection.content
;  (#set! injection.language "html")
;  (#set! injection.include-children)
;  (#set! injection.combine))

;(html_inner_text
;  text: (source_file) @injection.content
;  (#set! injection.language "html")
;  (#set! injection.include-children)
;  (#set! injection.combine))

(rust_expr_simple
  expr: (source_file) @injection.content
  (#set! injection.language "rust")
  (#set! injection.include-children))

(rust_expr_paren
  expr: (source_file) @injection.content
  (#set! injection.language "rust")
  (#set! injection.include-children))

(if_stmt
  head: (source_file) @injection.content
  (#set! injection.language "rust")
  (#set! injection.include-children))
(else_clause
  head: (source_file) @injection.content
  (#set! injection.language "rust")
  (#set! injection.include-children))

(for_stmt
  head: (source_file) @injection.content
  (#set! injection.language "rust")
  (#set! injection.include-children))

(while_stmt
  head: (source_file) @injection.content
  (#set! injection.language "rust")
  (#set! injection.include-children))

(match_stmt
  head: (source_file) @injection.content
  (#set! injection.language "rust")
  (#set! injection.include-children))
(match_stmt_arm
  pattern: (source_file) @injection.content
  (#set! injection.language "rust")
  (#set! injection.include-children))

(match_text
  text: (source_file) @injection.content
  (#set! injection.language "html")
  (#set! injection.include-children))

(raw_content
  content: (source_file) @injection.content
  (#set! injection.language "html")
  (#set! injection.include-children))

(rust_block
  content: (source_file) @injection.content
  (#set! injection.language "rust")
  (#set! injection.include-children))

(text_multiline
  text: (source_file) @injection.content
  (#set! injection.language "html")
  (#set! injection.include-children))

(text_line
  text: (source_file) @injection.content
  (#set! injection.language "html")
  (#set! injection.include-children))