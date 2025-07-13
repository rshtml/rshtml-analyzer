(start_symbol) @operator
(hash_symbol) @operator
(hash_symbol) @punctuation.special

(open_paren) @punctuation.bracket
(close_paren) @punctuation.bracket
(open_brace) @punctuation.bracket
(close_brace) @punctuation.bracket
;(open_bracket) @punctuation.bracket
;(close_bracket) @punctuation.bracket

;(comma) @punctuation.delimiter
(fat_arrow) @operator
(colon) @punctuation.delimiter
(semicolon) @punctuation.delimiter
(at_colon) @operator
(equals) @punctuation.delimiter

(string_line) @string

(comment_block) @comment
(open_comment) @operator
(close_comment) @operator

(continue_) @keyword
(break_) @keyword

(extends_) @keyword
(raw_) @keyword

(include_) @keyword
(render_) @keyword
(render_body_) @keyword
(child_content_) @keyword
(section_) @keyword

(section_block
  name: (rust_identifier) @namespace)

(use_) @keyword
(as_) @keyword

(number) @number
(bool) @boolean

(component
  name: (rust_identifier) @type)

(component_parameter
  name: (rust_identifier) @variable.parameter)

(as_clause
  alias: (rust_identifier) @type)

(tag_open) @punctuation.bracket
(tag_close) @punctuation.bracket
(tag_end_open) @punctuation.bracket
(tag_self_close) @punctuation.bracket

(component_tag
  name: (component_tag_identifier) @type)

(component_tag
  name_close: (component_tag_identifier) @type)

(component_tag_parameter
  name: (rust_identifier) @variable.parameter)

;this is for now extra
(else_clause
  head: (source_file) @keyword)
