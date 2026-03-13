.PHONY: lint test regen-fixtures regen-golden

lint:
	flake8 .

test:
	pytest

regen-fixtures:
	python scripts/regen_fixtures.py

regen-golden:
	python scripts/regen_golden.py
