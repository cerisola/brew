# DO NOT EDIT MANUALLY
# This is an autogenerated file for types exported from the `rspec_junit_formatter` gem.
# Please instead update this file by running `bin/tapioca gem rspec_junit_formatter`.

# typed: true

class RSpecJUnitFormatter < ::RSpec::Core::Formatters::BaseFormatter
  def dump_summary(notification); end
  def start(notification); end
  def stop(notification); end

  private

  def classname_for(notification); end
  def description_for(notification); end
  def duration; end
  def duration_for(notification); end
  def escape(text); end
  def example_count; end
  def example_group_file_path_for(notification); end
  def examples; end
  def exception_for(notification); end
  def failure_count; end
  def failure_for(notification); end
  def failure_message_for(example); end
  def failure_type_for(example); end
  def pending_count; end
  def result_of(notification); end
  def started; end
  def stderr_for(example_notification); end
  def stdout_for(example_notification); end
  def strip_diff_colors(string); end
  def swap_rspec_configuration(key, value); end
  def without_color(&block); end
  def xml_dump; end
  def xml_dump_example(example); end
  def xml_dump_examples; end
  def xml_dump_failed(example); end
  def xml_dump_output(example); end
  def xml_dump_pending(example); end
end

RSpecJUnitFormatter::DISCOURAGED_REGEXP = T.let(T.unsafe(nil), Regexp)
RSpecJUnitFormatter::DISCOURAGED_REPLACEMENTS = T.let(T.unsafe(nil), Hash)
RSpecJUnitFormatter::ILLEGAL_REGEXP = T.let(T.unsafe(nil), Regexp)
RSpecJUnitFormatter::ILLEGAL_REPLACEMENT = T.let(T.unsafe(nil), Hash)
RSpecJUnitFormatter::STRIP_DIFF_COLORS_BLOCK_REGEXP = T.let(T.unsafe(nil), Regexp)
RSpecJUnitFormatter::STRIP_DIFF_COLORS_CODES_REGEXP = T.let(T.unsafe(nil), Regexp)
RspecJunitFormatter = RSpecJUnitFormatter
