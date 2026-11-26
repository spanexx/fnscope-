package demo

/** Result carries a stable error code and message. */
data class Result(val code: Int, val message: String)

/** Greeter formats a greeting with a configurable prefix. */
class Greeter(val prefix: String = "Hello") {
    /** Build a greeter for a chosen prefix. */
    companion object {
        fun of(prefix: String = "Hi"): Greeter = Greeter(prefix)
    }

    /** Format a greeting for one name. */
    fun greet(name: String): String = "$prefix, $name"
}

/**
 * Echo returns its input.
 * @param text value to echo
 */
fun echo(text: String = "world"): String = text

/** Evaluate validates the input and builds a result. */
fun evaluate(input: Int): Result {
    require(input >= 0) { "INVALID_INPUT" }
    if (input == 0) {
        throw IllegalArgumentException("EMPTY_SENTENCE")
    }
    val greeter = Greeter.of("Hello")
    val message = greeter.greet(echo("Kotlin"))
    println(message)
    return Result(input, message)
}
