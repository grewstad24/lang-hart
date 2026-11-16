#include <napi.h>

typedef struct TSLanguage TSLanguage;

extern "C" const TSLanguage *tree_sitter_hart();

// "tree-sitter", "language" hashed with MD5
const napi_type_tag LANGUAGE_TYPE_TAG = {
  0x8AF2EA410CCD4CE6, 0xB385E79A638C1A3C
};

Napi::Object Init(Napi::Env env, Napi::Object exports) {
    exports["name"] = Napi::String::New(env, "hart");
    auto language = Napi::External<TSLanguage>::New(env, const_cast<TSLanguage *>(tree_sitter_hart()));
    language.TypeTag(&LANGUAGE_TYPE_TAG);
    exports["language"] = language;
    return exports;
}

NODE_API_MODULE(tree_sitter_hart_binding, Init)

