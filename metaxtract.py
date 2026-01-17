import sys
import cli

def main() -> int:
    return cli.main(sys.argv[1:])

if __name__ == "__main__":
    raise SystemExit(main())
