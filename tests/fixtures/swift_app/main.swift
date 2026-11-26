import Foundation

/// Result of grading one submission.
struct Grade {
    var score: Int
    var role: String
}

/// Normalizes raw text before grading.
func normalize(_ text: String) -> String {
    return text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
}

/// Grades one submission against a minimum score.
func grade(text: String, minimum: Int = 1) throws -> Grade {
    let clean = normalize(text)
    guard !clean.isEmpty else {
        throw FunctionError(code: "EMPTY_TEXT")
    }
    if clean.count < minimum {
        throw ValidationError.tooShort
    }
    print("graded \(clean)")
    return Grade(score: clean.count, role: clean)
}

extension Grade {
    /// Human-readable summary of one grade.
    func describe() -> String {
        return "\(role): \(score)"
    }
}

/// Prints one grade to the console.
func report(_ result: Grade) {
    print(result.describe())
}

/// Entry point used by the fixture's call-edge check.
func main() {
    let result = try? grade(text: "Example")
    report(result ?? Grade(score: 0, role: "none"))
}
