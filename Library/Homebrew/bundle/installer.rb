# typed: strict
# frozen_string_literal: true

require "bundle/dsl"
require "bundle/package_types"
require "bundle/skipper"

module Homebrew
  module Bundle
    module Installer
      class InstallableEntry < T::Struct
        const :name, String
        const :options, Homebrew::Bundle::EntryOptions
        const :verb, String
        const :cls, T.class_of(Homebrew::Bundle::PackageType)
      end

      sig {
        params(
          entries:    T::Array[Dsl::Entry],
          global:     T::Boolean,
          file:       T.nilable(String),
          no_lock:    T::Boolean,
          no_upgrade: T::Boolean,
          verbose:    T::Boolean,
          force:      T::Boolean,
          quiet:      T::Boolean,
        ).returns(T::Boolean)
      }
      def self.install!(entries, global: false, file: nil, no_lock: false, no_upgrade: false, verbose: false,
                        force: false, quiet: false)
        success = 0
        failure = 0

        installable_entries = entries.filter_map do |entry|
          next if Homebrew::Bundle::Skipper.skip? entry

          name = entry.name
          options = entry.options
          type = entry.type
          cls = Homebrew::Bundle.installable(type)
          next if cls.nil? || !cls.install_supported?

          InstallableEntry.new(name:, options:, verb: cls.install_verb(name, options), cls:)
        end

        if (fetchable_names = fetchable_formulae_and_casks(installable_entries, no_upgrade:).presence)
          fetchable_names_joined = fetchable_names.join(", ")
          puts Formatter.success("Fetching #{fetchable_names_joined}") unless quiet
          unless Bundle.brew("fetch", *fetchable_names, verbose:)
            $stderr.puts Formatter.error "`brew bundle` failed! Failed to fetch #{fetchable_names_joined}"
            return false
          end
        end

        installable_entries.each do |entry|
          name = entry.name
          options = entry.options
          verb = entry.verb
          cls = entry.cls

          preinstall = if cls.preinstall!(name, **options, no_upgrade:, verbose:)
            puts Formatter.success("#{verb} #{name}")
            true
          else
            puts "Using #{name}" unless quiet
            false
          end

          if cls.install!(name, **options,
                         preinstall:, no_upgrade:, verbose:, force:)
            success += 1
          else
            $stderr.puts Formatter.error("#{verb} #{name} has failed!")
            failure += 1
          end
        end

        unless failure.zero?
          require "utils"
          dependency = Utils.pluralize("dependency", failure)
          $stderr.puts Formatter.error "`brew bundle` failed! #{failure} Brewfile #{dependency} failed to install"
          return false
        end

        unless quiet
          require "utils"
          dependency = Utils.pluralize("dependency", success)
          puts Formatter.success "`brew bundle` complete! #{success} Brewfile #{dependency} now installed."
        end

        true
      end

      sig {
        params(
          entries:    T::Array[InstallableEntry],
          no_upgrade: T::Boolean,
        ).returns(T::Array[String])
      }
      def self.fetchable_formulae_and_casks(entries, no_upgrade:)
        entries.filter_map do |entry|
          entry.cls.fetchable_name(entry.name, entry.options, no_upgrade:)
        end
      end
    end
  end
end
