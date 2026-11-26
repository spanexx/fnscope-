import java.nio.file.Files;
import java.nio.file.Paths;

/** A small standalone Java fixture for the fnscope Java adapter. */
public final class Main {
    /** Result of evaluating a role. */
    public static class RoleResult {
        public String role;
        public double score;
    }

    /**
     * Normalizes the supplied text.
     */
    // normalize trims text and rejects empty input.
    static String normalize(String text) {
        if (text.isEmpty()) {
            throw new IllegalArgumentException("EMPTY_TEXT");
        }
        return text.trim();
    }

    // analyze validates, normalizes and logs the cleaned value.
    static RoleResult analyze(String text) {
        // Guard against blank input before doing any work.
        if (text.isBlank()) {
            throw new FunctionError("TOKEN_NOT_FOUND");
        }
        String cleaned = normalize(text);
        System.out.println("cleaned: " + cleaned);
        RoleResult result = new RoleResult();
        result.role = cleaned;
        return result;
    }

    // readConfig loads a local file and reports its size.
    static int readConfig() throws Exception {
        String body = Files.readString(Paths.get("config.txt"));
        return body.length();
    }

    // auth reads an API key from the environment.
    static String auth() {
        return System.getenv("API_KEY");
    }

    // run drives the fixture and produces the call edge into analyze.
    public static void run(String... args) {
        String output = analyze("hello").role;
        System.out.println(output);
    }

    public static void main(String[] args) {
        run(args);
    }
}

class FunctionError extends RuntimeException {
    FunctionError(String code) {
        super(code);
    }
}
