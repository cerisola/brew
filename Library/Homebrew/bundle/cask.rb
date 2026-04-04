# typed: true # rubocop:todo Sorbet/StrictSigil
# frozen_string_literal: true

require "utils/output"
require "bundle/package_type"

module Homebrew
  module Bundle
    class Cask < Homebrew::Bundle::PackageType
      extend ::Utils::Output::Mixin

      PACKAGE_TYPE = :cask
      PACKAGE_TYPE_NAME = "Cask"

      class << self
        def reset!
          @casks = nil
          @cask_names = nil
          @cask_oldnames = nil
          @installed_casks = nil
          @outdated_casks = nil
        end

        private

        def upgrading?(no_upgrade, name, options)
          return false if no_upgrade
          return true if cask_upgradable?(name)
          return false unless options[:greedy]

          cask_is_outdated_using_greedy?(name)
        end

        def postinstall_change_state!(name:, options:, verbose:)
          postinstall = options.fetch(:postinstall, nil)
          return true if postinstall.blank?

          puts "Running postinstall for #{@name}: #{postinstall}" if verbose
          Kernel.system(postinstall)
        end

        def casks
          return [] unless Bundle.cask_installed?

          require "cask/caskroom"
          @casks ||= ::Cask::Caskroom.casks
        end

        def explicit_s(cask_config)
          cask_config.explicit.map do |key, value|
            # inverse of #env - converts :languages config key back to --language flag
            if key == :languages
              key = "language"
              value = Array(cask_config.explicit.fetch(:languages, [])).join(",")
            end
            "#{key}: \"#{value.to_s.sub(/^#{Dir.home}/, "~")}\""
          end.join(", ")
        end

        public

        def install_verb(name, options = {})
          return "Installing" if !cask_installed?(name) || !upgrading?(false, name, options)

          "Upgrading"
        end

        def preinstall!(name, no_upgrade: false, verbose: false, **options)
          if cask_installed?(name) && !upgrading?(no_upgrade, name, options)
            puts "Skipping install of #{name} cask. It is already installed." if verbose
            return false
          end

          true
        end

        def install!(name, preinstall: true, no_upgrade: false, verbose: false, force: false, **options)
          return true unless preinstall

          full_name = options.fetch(:full_name, name)

          install_result = if cask_installed?(name) && upgrading?(no_upgrade, name, options)
            status = "#{options[:greedy] ? "may not be" : "not"} up-to-date"
            puts "Upgrading #{name} cask. It is installed but #{status}." if verbose
            Bundle.brew("upgrade", "--cask", full_name, verbose:)
          else
            args = options.fetch(:args, []).filter_map do |k, v|
              case v
              when TrueClass
                "--#{k}"
              when FalseClass, NilClass
                nil
              else
                "--#{k}=#{v}"
              end
            end

            args << "--force" if force
            args << "--adopt" unless args.include?("--force")
            args.uniq!

            with_args = " with #{args.join(" ")}" if args.present?
            puts "Installing #{name} cask#{with_args}. It is not currently installed." if verbose

            if Bundle.brew("install", "--cask", full_name, *args, verbose:)
              installed_casks << name
              true
            else
              false
            end
          end
          result = install_result

          if cask_installed?(name)
            postinstall_result = postinstall_change_state!(name:, options:, verbose:)
            result &&= postinstall_result
          end

          result
        end

        def installable_or_upgradable?(name, no_upgrade: false, **options)
          !cask_installed?(name) || upgrading?(no_upgrade, name, options)
        end

        def fetchable_name(name, options = {}, no_upgrade: false)
          full_name = T.cast(options.fetch(:full_name, name), String)
          user, repository, = full_name.split("/", 3)
          return if user.present? && repository.present? &&
                    Homebrew::Bundle::Tap.installed_taps.exclude?("#{user}/#{repository}")
          return unless installable_or_upgradable?(name, no_upgrade:, **options)

          full_name
        end

        def cask_installed_and_up_to_date?(cask, no_upgrade: false)
          return false unless cask_installed?(cask)
          return true if no_upgrade

          !cask_upgradable?(cask)
        end

        def cask_in_array?(cask, array)
          return true if array.include?(cask)

          array.include?(cask.split("/").last)
        end

        def cask_installed?(cask)
          return true if cask_in_array?(cask, installed_casks)

          old_name = cask_oldnames[cask]
          old_name ||= cask_oldnames[cask.split("/").last]
          return false unless old_name
          return false unless cask_in_array?(old_name, installed_casks)

          opoo "#{cask} was renamed to #{old_name}"

          true
        end

        def cask_upgradable?(cask)
          cask_in_array?(cask, outdated_casks)
        end

        def installed_casks
          @installed_casks ||= cask_names
        end

        def outdated_casks
          @outdated_casks ||= outdated_cask_names
        end

        def cask_names
          @cask_names ||= casks.map(&:to_s)
        end

        def outdated_cask_names
          return [] unless Bundle.cask_installed?

          casks.select { |c| c.outdated?(greedy: false) }
               .map(&:to_s)
        end

        def cask_is_outdated_using_greedy?(cask_name)
          return false unless Bundle.cask_installed?

          cask = casks.find { |installed_cask| installed_cask.to_s == cask_name }
          return false if cask.nil?

          cask.outdated?(greedy: true)
        end

        def dump(describe: false)
          casks.map do |cask|
            description = "# #{cask.desc}\n" if describe && cask.desc.present?
            config = ", args: { #{explicit_s(cask.config)} }" if cask.config.present? && cask.config.explicit.present?
            "#{description}cask \"#{cask.full_name}\"#{config}"
          end.join("\n")
        end

        def dump_output(describe: false, no_restart: false)
          _ = no_restart

          dump(describe:)
        end

        def cask_oldnames
          @cask_oldnames ||= casks.each_with_object({}) do |c, hash|
            oldnames = c.old_tokens
            next if oldnames.blank?

            oldnames.each do |oldname|
              hash[oldname] = c.full_name
              if c.full_name.include? "/" # tap cask
                tap_name = c.full_name.rpartition("/").first
                hash["#{tap_name}/#{oldname}"] = c.full_name
              end
            end
          end
        end

        def formula_dependencies(cask_list)
          return [] unless Bundle.cask_installed?
          return [] if cask_list.blank?

          casks.flat_map do |cask|
            next unless cask_list.include?(cask.to_s)

            cask.depends_on[:formula]
          end.compact
        end
      end

      def installed_and_up_to_date?(cask, no_upgrade: false)
        self.class.cask_installed_and_up_to_date?(cask, no_upgrade:)
      end
    end
  end
end
