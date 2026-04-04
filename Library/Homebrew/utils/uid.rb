# typed: strict
# frozen_string_literal: true

module Utils
  module UID
    sig { returns(T.nilable(String)) }
    def self.uid_home
      require "etc"
      Etc.getpwuid(Process.uid)&.dir
    rescue ArgumentError
      # Cover for misconfigured NSS setups
      nil
    end
  end
end
