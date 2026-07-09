
[group('release')]
release-tag version:
  rellog ready {{ version }}

  git tag -a {{ version }} -m "Release {{ version }}"
