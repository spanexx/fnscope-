# Small coherent Ruby app used as the fnscope Ruby adapter fixture.
# It exercises discovery, a downstream call edge, a guarded raise, a class
# method, a side effect and a struct type index entry.

# Normalize user-supplied text before it is scored.
# @param [String] text raw input
# @return [String] cleaned text
def normalize_text(text)
  cleaned = strip_controls(text)
  if cleaned.empty?
    raise ArgumentError, "EMPTY_TEXT"
  end
  cleaned
end

# Remove surrounding whitespace without touching visible content.
# @param [String] text raw input
# @return [String] cleaned text
def strip_controls(text)
  text.strip
end

# Report a verdict to the console.
def report(text)
  puts text
end

# Format values for display.
class Formatter
  attr_reader :prefix

  # @param [String] value raw value
  # @return [String] formatted value
  def format(value)
    "#{prefix}: #{value}"
  end
end

# Build a formatter with the default prefix.
# @return [Formatter] a fresh formatter
def self.default_formatter
  Formatter.new
end

Record = Struct.new(:id, :name)
