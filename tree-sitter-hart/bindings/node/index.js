const root = require("path").join(__dirname, "..", "..");

module.exports =
  typeof process.versions.node !== "undefined"
    ? require("node-gyp-build")(root)
    : require("./binding.js");

try {
  module.exports.nodeTypeInfo = require("../../src/node-types.json");
} catch (_) {}

