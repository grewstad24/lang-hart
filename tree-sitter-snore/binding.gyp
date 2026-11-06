{
  "targets": [
    {
      "target_name": "tree_sitter_snore_binding",
      "dependencies": [
        "<!(node -p \"require('node-addon-api').gyp\")"
      ],
      "sources": [
        "bindings/node/binding.cc",
        "src/parser.c"
      ],
      "cflags_c": [
        "-std=c11",
      ]
    }
  ]
}

