// swift-tools-version:5.9
import PackageDescription
import Foundation

// Languages this package vends as plugin frameworks. Each one corresponds to a
// MarkdawnLang{Pascal}.xcframework in ../crates/markdawn-swift/ produced by
// build-xcframework.sh.
//
// Consuming apps depend on only the plugin products they want; each adds
// ~500 KB – 10 MB to the app bundle for its grammar. Grammars are loaded
// at runtime via Markdawn.loadLanguage(_:).
//
// Keep in sync with the output of crates/markdawn-swift/generate-plugins.sh.
let languages: [String] = [
  "rust", "swift", "json", "javascript", "python",
]

func pascalCase(_ name: String) -> String {
  name.split(separator: "-").map { String($0).capitalized }.joined()
}

let langTargets: [Target] = languages.flatMap { name -> [Target] in
  let pascal = pascalCase(name)
  let product = "MarkdawnLang\(pascal)"
  return [
    .binaryTarget(
      name: "\(product)FFI",
      path: "../crates/markdawn-swift/\(product).xcframework"
    ),
    .target(
      name: product,
      dependencies: ["Markdawn", .target(name: "\(product)FFI")],
      path: "Sources/\(product)"
    ),
  ]
}

let langProducts: [Product] = languages.map { name in
  let pascal = pascalCase(name)
  return .library(name: "MarkdawnLang\(pascal)", targets: ["MarkdawnLang\(pascal)"])
}

let package = Package(
  name: "Markdawn",
  platforms: [.iOS(.v15), .macCatalyst(.v15)],
  products: [
    .library(name: "Markdawn", targets: ["Markdawn"]),
  ] + langProducts,
  targets: [
    .binaryTarget(
      name: "MarkdawnFFI",
      path: "../crates/markdawn-swift/Markdawn.xcframework"
    ),
    .target(
      name: "Markdawn",
      dependencies: ["MarkdawnFFI"],
      path: "Sources/Markdawn"
    ),
    .testTarget(
      name: "MarkdawnTests",
      dependencies: ["Markdawn", "MarkdawnLangRust"],
      path: "Tests/MarkdawnTests"
    ),
  ] + langTargets
)
