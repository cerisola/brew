# typed: strict
# frozen_string_literal: true

require "bundle/extensions/extension"

module Homebrew
  module Bundle
    class Krew < Extension
      PACKAGE_TYPE = :krew
      PACKAGE_TYPE_NAME = "Krew Plugin"
      BANNER_NAME = "Krew plugins"

      class << self
        sig { override.void }
        def reset!
          @packages = T.let(nil, T.nilable(T::Array[String]))
          @installed_packages = T.let(nil, T.nilable(T::Array[String]))
          @package_manager_executable = T.let(nil, T.nilable(Pathname))
          @krew_installed = T.let(nil, T.nilable(T::Boolean))
        end

        sig { override.returns(T.nilable(Pathname)) }
        def package_manager_executable
          @package_manager_executable ||= T.let(which("kubectl", ORIGINAL_PATHS), T.nilable(Pathname))
        end

        sig { override.returns(T::Boolean) }
        def package_manager_installed?
          return @krew_installed unless @krew_installed.nil?

          kubectl = package_manager_executable
          result = if kubectl.present?
            Kernel.system(package_manager_env(kubectl), kubectl.to_s, "krew", "version",
                          out: File::NULL, err: File::NULL) == true
          else
            false
          end
          @krew_installed = T.let(result, T.nilable(T::Boolean))
          result
        end

        sig { override.returns(T::Array[String]) }
        def packages
          packages = @packages
          return packages if packages

          @packages = if package_manager_installed?
            with_package_manager_env do |kubectl|
              parse_plugin_list(`#{kubectl} krew list 2>/dev/null`)
            end
          else
            []
          end
        end

        sig {
          override.params(
            name:    String,
            with:    T.nilable(T::Array[String]),
            verbose: T::Boolean,
          ).returns(T::Boolean)
        }
        def install_package!(name, with: nil, verbose: false)
          _ = with

          with_package_manager_env do |kubectl|
            Bundle.system(kubectl.to_s, "krew", "install", name, verbose:)
          end
        end

        sig { override.returns(T::Array[String]) }
        def installed_packages
          installed_packages = @installed_packages
          return installed_packages if installed_packages

          @installed_packages = packages.dup
        end

        sig { params(output: String).returns(T::Array[String]) }
        def parse_plugin_list(output)
          output.lines.filter_map do |line|
            line = line.strip
            next if line.empty?

            name = line.split(/\s+/).first
            name.presence
          end.uniq
        end
        private :parse_plugin_list
      end
    end
  end
end
