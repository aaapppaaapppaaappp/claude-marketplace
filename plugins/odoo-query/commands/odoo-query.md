---
name: odoo-query
description: Connect to an Odoo instance via XML-RPC and perform read-only queries to investigate issues, explore data structures, and find solutions. Use when user wants to "query odoo", "check odoo data", "investigate odoo", "explore odoo models", "debug odoo issue".
arguments:
  - name: url
    description: Odoo instance URL (e.g., https://mycompany.odoo.com)
    required: true
  - name: database
    description: Database name
    required: true
  - name: login
    description: Username or email
    required: true
  - name: api_key
    description: API key or password
    required: true
  - name: query
    description: What to investigate (optional, can be provided interactively)
    required: false
---

# Odoo Query Command

Connect to Odoo via XML-RPC and perform **READ-ONLY** queries to investigate issues.

## CRITICAL SECURITY RULES

```
╔══════════════════════════════════════════════════════════════════════════════╗
║  ONLY READ OPERATIONS ARE ALLOWED                                            ║
║  - search, search_read, read, fields_get, search_count                       ║
║  - NEVER use: create, write, unlink, or any method that modifies data        ║
║  - NEVER execute arbitrary code or methods                                   ║
╚══════════════════════════════════════════════════════════════════════════════╝
```

## Connection Setup

Use the Python script at `${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py` to connect:

```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "URL" \
  --db "DATABASE" \
  --login "LOGIN" \
  --api-key "API_KEY" \
  --action ACTION \
  [--model MODEL] \
  [--domain DOMAIN] \
  [--fields FIELDS] \
  [--limit LIMIT]
```

## Available Actions

### 1. Test Connection
```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action test
```

### 2. List All Models
```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action list_models
```

### 3. Get Model Fields
```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action fields_get \
  --model "sale.order"
```

### 4. Search Records
```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action search_read \
  --model "sale.order" \
  --domain "[('state', '=', 'sale')]" \
  --fields "name,partner_id,amount_total,state" \
  --limit 10
```

### 5. Count Records
```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action search_count \
  --model "sale.order" \
  --domain "[('state', '=', 'draft')]"
```

### 6. Read Specific Records
```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action read \
  --model "sale.order" \
  --ids "1,2,3" \
  --fields "name,partner_id,order_line"
```

### 7. Describe Model (Comprehensive)
Get everything about a model in one call: all fields with types, selection values from ir.model.fields (not fields_get which returns None), required fields highlighted, and relational field targets.

```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action describe_model \
  --model "sale.order"
```

**Why use this instead of fields_get?**
- Consolidates multiple calls into one
- Returns selection values from ir.model.fields (fields_get returns None)
- Lists required fields separately for migration planning
- Shows relational field targets clearly

### 8. Find Model (Fuzzy Search)
Search for models by keyword across model name and description, with record counts to understand their usage.

```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action find_model \
  --keyword "cycle"
```

**Why use this?**
- Avoid confusion about model purposes (e.g., is this catalog or student-facing?)
- Record counts help identify active vs. unused models
- Fuzzy search finds models even with partial keywords

## Execution Flow

### Step 1: Get Connection Details
If not provided, ask the user for:
- URL (Odoo instance URL)
- Database name
- Login (username/email)
- API Key (from Settings > Users > API Keys)

### Step 2: Test Connection
Always test the connection first:
```bash
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action test
```

### Step 3: Investigate Based on User Query

**For "show me all models" or exploring structure:**
```bash
--action list_models
```

**For "what fields does X have":**
```bash
--action describe_model --model "model.name"
# Or use fields_get for basic info without selection values
--action fields_get --model "model.name"
```

**For "find models related to...":**
```bash
--action find_model --keyword "invoice"
```

**For "find records where...":**
```bash
--action search_read --model "model.name" --domain "[...]" --fields "field1,field2" --limit 20
```

**For "how many records...":**
```bash
--action search_count --model "model.name" --domain "[...]"
```

## Domain Filter Syntax

Odoo domains use Polish notation (prefix):
- `[('field', '=', 'value')]` - equals
- `[('field', '!=', 'value')]` - not equals
- `[('field', 'in', ['a', 'b'])]` - in list
- `[('field', 'like', '%pattern%')]` - SQL LIKE
- `[('field', 'ilike', '%pattern%')]` - case-insensitive LIKE
- `[('field', '>', 10)]` - greater than
- `[('field', '>=', 10)]` - greater or equal
- `[('field', '<', 10)]` - less than
- `[('field', '<=', 10)]` - less or equal
- `['|', ('field1', '=', 'a'), ('field2', '=', 'b')]` - OR condition
- `['&', ('field1', '=', 'a'), ('field2', '=', 'b')]` - AND condition (default)
- `[('field', '=', False)]` - is empty/null
- `[('field', '!=', False)]` - is not empty

## Common Investigation Patterns

### Check Sale Orders
```bash
# Recent sales
--action search_read --model "sale.order" \
  --domain "[('state', '=', 'sale')]" \
  --fields "name,partner_id,date_order,amount_total" \
  --limit 10

# Draft quotes
--action search_read --model "sale.order" \
  --domain "[('state', '=', 'draft')]" \
  --fields "name,partner_id,create_date"
```

### Check Invoices
```bash
# Unpaid invoices
--action search_read --model "account.move" \
  --domain "[('move_type', '=', 'out_invoice'), ('payment_state', '!=', 'paid')]" \
  --fields "name,partner_id,amount_total,invoice_date_due"
```

### Check Stock
```bash
# Products with low stock
--action search_read --model "stock.quant" \
  --domain "[('quantity', '<', 10)]" \
  --fields "product_id,location_id,quantity"
```

### Check Users/Partners
```bash
# Active users
--action search_read --model "res.users" \
  --domain "[('active', '=', True)]" \
  --fields "name,login,groups_id"

# Customers
--action search_read --model "res.partner" \
  --domain "[('customer_rank', '>', 0)]" \
  --fields "name,email,phone,country_id"
```

### Check Custom Models
```bash
# First, get the model structure
--action fields_get --model "custom.model.name"

# Then query with appropriate fields
--action search_read --model "custom.model.name" \
  --domain "[]" \
  --fields "name,state" \
  --limit 5
```

## Output Format

The script outputs JSON for easy parsing. Example:
```json
{
  "success": true,
  "action": "search_read",
  "model": "sale.order",
  "count": 3,
  "data": [
    {"id": 1, "name": "SO001", "amount_total": 1500.0},
    {"id": 2, "name": "SO002", "amount_total": 2300.0}
  ]
}
```

## Error Handling

If connection fails, check:
1. URL is correct and accessible
2. Database name is exact
3. Login credentials are valid
4. API key has proper permissions
5. User has access to requested models

## Example Session

```
User: Connect to our Odoo and show me unpaid invoices