.PHONY: lint test regen-fixtures

lint:
	flake8 .

test:
	pytest

regen-fixtures:
	python scripts/regen_fixtures.py
