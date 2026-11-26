using System;
using System.IO;
using System.Net.Http;

namespace CSharpApp
{
    /// <summary>Result of evaluating a role.</summary>
    public sealed class RoleResult
    {
        /// <summary>Selected role name.</summary>
        public string Role { get; set; }

        /// <summary>Confidence score.</summary>
        public double Score;

        public int Count;
        public int Total;

        public RoleResult(string role, double score)
        {
            Role = role;
            Score = score;
        }
    }

    /// <summary>A small primary-constructor record used by the type index.</summary>
    public sealed record Ping(string Code, int Retry = 3);

    public static class TextTools
    {
        // normalize trims the supplied text and rejects an empty value.
        public static string Normalize(string text)
        {
            if (string.IsNullOrWhiteSpace(text))
            {
                throw new FunctionError("EMPTY_TEXT");
            }
            return text.Trim();
        }

        // add adds a configurable factor to a value.
        public static double Add(double value, double factor = 2.5)
        {
            return value + factor;
        }

        // analyze validates, normalizes and logs the cleaned value.
        public static RoleResult Analyze(string text)
        {
            // Guard against an empty string before doing any work.
            if (text.Length == 0)
            {
                throw new FunctionError("TOKEN_NOT_FOUND");
            }
            string cleaned = Normalize(text);
            double scaled = Add(cleaned.Length);
            Console.WriteLine("cleaned: " + cleaned);
            RoleResult result = new RoleResult(cleaned, scaled);
            return result;
        }

        // readConfig loads a local file and reports its size.
        public static int ReadConfig()
        {
            string body = File.ReadAllText("config.txt");
            return body.Length;
        }

        // auth reads an API key from the environment.
        public static string Auth()
        {
            return Environment.GetEnvironmentVariable("API_KEY");
        }

        // fetch downloads a remote resource.
        public static string Fetch(string url)
        {
            using var client = new HttpClient();
            return client.GetStringAsync(url).Result;
        }

        // run drives the fixture and declares a first-level local function.
        public static string Run(string value)
        {
            string Localize(string input)
            {
                string Inner(string nested)
                {
                    return nested;
                }
                return Inner(input);
            }
            string localized = Localize(value);
            return localized;
        }
    }

    public sealed class FunctionError : Exception
    {
        public FunctionError(string code) : base(code)
        {
        }
    }
}
