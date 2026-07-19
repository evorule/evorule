import sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from tests.test_e2e import main
import asyncio
rc = asyncio.run(main())
sys.exit(rc)
